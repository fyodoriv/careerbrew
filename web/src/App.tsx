import { Fragment, useEffect, useState } from "react";
import { ActionsView } from "./actions";
import { api, captureToken } from "./api";
import { startLiveRefresh } from "./live";
import { ReportsView } from "./reports";
import { isBlocked, navGroups, personalOnly, type Role, useRoute, VIEWS } from "./router";
import { NotificationsView } from "./notifications";
import { useSession } from "./session";
import { SignInPage } from "./signin";
import { StandupView } from "./standup";
import { GoalsView, TeachView } from "./teach";
import { DashboardView, InterviewsView, JobsView, LibraryView, Loading, PracticeView, ReviewView, SetupView, TodayView, WeekView, WorkView } from "./views";

captureToken();

export default function App() {
  const route = useRoute();
  const session = useSession();
  const signedIn = session.state === "ok";
  const [role, setRole] = useState<Role>(undefined);  // undefined: the role is still loading
  useEffect(() => {
    if (signedIn) api.overview().then((o) => setRole(o.role)).catch(() => setRole(null));
  }, [signedIn, session.epoch]);
  useEffect(() => (signedIn ? startLiveRefresh() : undefined), [signedIn]);
  const current = VIEWS.find((v) => v.id === route.view)!;
  useEffect(() => {
    document.title = `${current.label} · careerbrew`;
  }, [current.label]);
  // On a phone the views sit behind one Menu button (comfort budget, goal 2026-10-22); opening a view closes it.
  const [menuOpen, setMenuOpen] = useState(false);
  useEffect(() => setMenuOpen(false), [route.view, route.param]);
  const view = {
    dashboard: <DashboardView />,
    teach: <TeachView />,
    goals: <GoalsView />,
    today: <TodayView />,
    standup: <StandupView />,
    notifications: <NotificationsView focus={route.param} />,
    week: <WeekView />,
    jobs: <JobsView />,
    interviews: <InterviewsView company={route.param} />,
    practice: <PracticeView />,
    library: <LibraryView pageId={route.param} />,
    work: <WorkView />,
    review: <ReviewView />,
    reports: <ReportsView id={route.param} />,
    setup: <SetupView />,
    actions: <ActionsView name={route.param} />,
  }[route.view];
  // Job-hunt views answer 403 on a corporate-role machine by design: hide them, skip their fetches (#52, #65).
  const blocked = isBlocked(current, role);
  return (
    <div className="app">
      <aside className="side" data-menu={menuOpen ? "open" : "closed"}>
        <a className="brand" href="#/today"><span className="brand-mark" aria-hidden="true">cb</span>careerbrew</a>
        <button type="button" className="menu-toggle" aria-expanded={menuOpen} aria-controls="views-nav" onClick={() => setMenuOpen((o) => !o)}>
          <span aria-hidden="true">{menuOpen ? "✕" : "☰"}</span> Menu
        </button>
        <nav aria-label="Views" id="views-nav" onClick={(e) => (e.target as HTMLElement).closest("a") && setMenuOpen(false)}>
          {navGroups(VIEWS, role).map(({ group, views }) => (
            <div key={group} className="nav-group">
              <div className="nav-label">{group}</div>
              {views.map((v) => (
                <a key={v.id} href={`#/${v.id}`} aria-current={route.view === v.id ? "page" : undefined}>{v.label}</a>
              ))}
            </div>
          ))}
        </nav>
        <p className="side-foot">Local only · 127.0.0.1</p>
      </aside>
      <main>
        {!(route.view === "standup" && session.state === "ok" && !blocked) && <header className="page-head">
          <h1>{current.label}</h1>
          <p className="muted">{current.hint}</p>
        </header>}
        {/* One gap between every stacked block of every view (issue #142). */}
        <div className="stack view">
          {session.state === "checking" ? (
            <Loading label="your sign-in" />
          ) : session.state !== "ok" ? (
            <SignInPage state={session.state} view={route.view} onRetry={session.retry} />
          ) : blocked ? (
            <p className="callout" role="status">This view is not available here: job-hunt views are personal-only, and this is a corporate machine.</p>
          ) : personalOnly(current) && role === undefined ? (
            <Loading label={current.label.toLowerCase()} />
          ) : (
            <Fragment key={session.epoch}>{view}</Fragment>
          )}
        </div>
      </main>
    </div>
  );
}
