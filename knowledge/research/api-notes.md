# API notes: job sources, mail, calendar

Checked on 2026-10-18. "Live-probed" means a real unauthenticated request returned the listed field names on that date. Everything else comes from the cited official docs.

## Summary

| Source | Key needed | Usable for local / remote | Main restriction |
| --- | --- | --- | --- |
| Adzuna (per country) | Yes: `app_id` + `app_key` (free) | Yes: `where=<city>` | 250 calls/day default. Branding if you publish. |
| RemoteOK | No | Remote only | Link back to the job URL and name "Remote OK". |
| Remotive | No | Remote only | Max ~4 fetches/day. Over 2 req/min is blocked. Link back. 24 h delay. |
| Himalayas | No (optional free key) | Yes: `country=<name>` | Link back. Do not resubmit to other boards. Data refreshes daily. |
| HN Who is hiring | No | Free text, parse it | None stated. Be polite. |
| We Work Remotely RSS | No | Remote only | No published API terms. RSS only. |
| LinkedIn | No usable API | Use your own data export + alert emails | User Agreement bans scraping. |
| Wellfound, WTTJ/Otta, Indeed, Glassdoor | No public API | No | Scraping only, against their terms. |

---

## 1. Adzuna

- Docs: https://developer.adzuna.com/docs/search , https://developer.adzuna.com/overview , terms: https://developer.adzuna.com/docs/terms_of_service
- Endpoint: `GET https://api.adzuna.com/v1/api/jobs/{country}/search/{page}`
  - Set `country` to your own two-letter code. The docs only show `gb`. Verify other codes with the first real key.
  - Example: `https://api.adzuna.com/v1/api/jobs/gb/search/1?app_id=ID&app_key=KEY&what=software%20engineer&where=London&results_per_page=50&content-type=application/json`
- Params (documented): `app_id`, `app_key`, `what`, `what_exclude`, `where`, `results_per_page`, `sort_by` (e.g. `salary`, `date`), `salary_min`, `full_time=1`, `permanent=1`, `content-type=application/json`.
  - Also in the interactive docs (needs login to view): `distance` (km), `max_days_old`, `what_or`, `what_phrase`, `category`. Not verified in the public text.
- Auth: API key pair as query params. Register at developer.adzuna.com. Unknown key returns `{"exception":"AUTH_FAIL"}` (live-probed).
- Limits (terms): 25 hits/min, 250/day, 1,000/week, 2,500/month by default.
- Terms that matter:
  - Allowed uses: publishing listings, salary estimates, **personal research**. careerbrew is personal research.
  - Other commercial use: 14-day trial only.
  - If you show listings publicly: "Adzuna" label (min 116x23 px) linked to the local domain. For research: credit "The Adzuna API" with a link.
- Remote flag: none. Filter with `what=remote` or check `title`/`description`.
- Fields:

| Need | Field |
| --- | --- |
| id | `results[].id` |
| title | `results[].title` |
| company | `results[].company.display_name` |
| location | `results[].location.display_name`, `location.area[]`, `latitude`, `longitude` |
| remote | none |
| salary | `salary_min`, `salary_max`, `salary_is_predicted` ("1" = estimate) |
| url | `redirect_url` |
| posted | `created` (ISO 8601) |
| description | `description` (snippet only, not full text) |

```json
{"__CLASS__":"Adzuna::API::Response::JobSearchResults","count":1234,"mean":110000,
 "results":[{"id":"4321","title":"Senior Software Engineer","company":{"display_name":"Acme"},
  "location":{"display_name":"London, UK","area":["UK","London"]},
  "latitude":51.51,"longitude":-0.13,"salary_min":120000,"salary_max":150000,"salary_is_predicted":"0",
  "contract_time":"full_time","contract_type":"permanent","category":{"tag":"it-jobs","label":"IT Jobs"},
  "created":"2026-10-17T12:00:00Z","redirect_url":"https://www.adzuna.co.uk/land/ad/4321?...","description":"..."}]}
```

## 2. RemoteOK

- Docs: https://remoteok.com/api (the legal notice is item `[0]` of the response). Live-probed.
- Endpoint: `GET https://remoteok.com/api`. No search params are documented. Fetch all and filter locally (`tags`, `position`, `location`).
- Auth: none. Send a real `User-Agent`.
- Terms (quoted from item `[0].legal`, shortened): link back to the Remote OK job URL **with follow, not nofollow**, and mention Remote OK as the source, or API access is suspended. Do not use the logo.
- Limits: not stated. Cache; fetch a few times a day at most.
- Response: a JSON array. Item `[0]` is metadata (`last_updated`, `legal`). Jobs start at `[1]`.
- Remote flag: all jobs are remote. `location` holds region limits (e.g. "Worldwide", "Germany").

| Need | Field |
| --- | --- |
| id | `id` (also `slug`) |
| title | `position` |
| company | `company` |
| location | `location` |
| remote | implied (all) |
| salary | `salary_min`, `salary_max` (USD/yr, 0 = unknown) |
| url | `url` (Remote OK page — use this for the link-back), `apply_url` |
| posted | `date` (ISO 8601), `epoch` |
| description | `description` (HTML) |

```json
[{"last_updated":1792350002,"legal":"API Terms of Service: ..."},
 {"slug":"remote-senior-backend-engineer-acme-123","id":"123","epoch":1792340000,"date":"2026-10-17T10:00:00+00:00",
  "company":"Acme","company_logo":"","position":"Senior Backend Engineer","tags":["python","backend"],
  "logo":"","description":"<p>...</p>","location":"Germany","salary_min":120000,"salary_max":160000,
  "apply_url":"https://remoteok.com/remote-jobs/123","url":"https://remoteok.com/remote-jobs/123","verified":true}]
```

## 3. Remotive

- Docs: https://github.com/remotive-com/remote-jobs-api , terms: https://remotive.com/api-documentation . Live-probed.
- Endpoints:
  - `GET https://remotive.com/api/remote-jobs?category=software-development&search=python&limit=50`
  - `GET https://remotive.com/api/remote-jobs/categories` (valid slugs). Current dev slugs: `software-development`, `devops`, `data`, `qa`, `artificial-intelligence`, `engineering`.
- Params: `category` (name or slug), `company_name` (partial), `search` (title + description, partial), `limit`.
- Auth: none.
- Limits: max ~4 fetches/day advised. More than 2 requests/min are blocked.
- Terms (from the `0-legal-notice` field in every response):
  - Link back to the Remotive job URL and name Remotive as the source.
  - Do not submit jobs to third-party sites (Jooble, Google Jobs, LinkedIn Jobs, ...).
  - Jobs are delayed 24 hours.
  - Do not use the jobs to collect signups or emails.
- Remote flag: all remote. `candidate_required_location` holds region limits (e.g. "Germany", "USA", "Worldwide").

| Need | Field |
| --- | --- |
| id | `jobs[].id` |
| title | `title` |
| company | `company_name` |
| location | `candidate_required_location` |
| remote | implied (all) |
| salary | `salary` (free-text string, often empty) |
| url | `url` |
| posted | `publication_date` (ISO 8601, no TZ) |
| description | `description` (HTML) |

```json
{"00-warning":"...","0-legal-notice":"...","job-count":1,"total-job-count":1500,
 "jobs":[{"id":2000001,"url":"https://remotive.com/remote-jobs/software-dev/x-2000001","title":"Staff Engineer",
  "company_name":"Acme","company_logo":"...","category":"Software Development","tags":["go"],"job_type":"full_time",
  "publication_date":"2026-10-16T09:00:00","candidate_required_location":"Germany","salary":"$140k - $170k",
  "description":"<p>...</p>"}]}
```

## 4. Himalayas

- Docs: https://himalayas.app/api , terms: https://himalayas.app/terms#api . Live-probed.
- Endpoints:
  - Browse: `GET https://himalayas.app/jobs/api?limit=20&cursor=...` (max `limit` 20; pass back `nextCursor`; `offset` is deprecated).
  - Search: `GET https://himalayas.app/jobs/api/search?q=software%20engineer&country=Germany&sort=recent&page=1`
- Search params: `q`, `country` (ISO alpha-2 or name), `worldwide`, `exclude_worldwide`, `seniority`, `employment_type`, `company` (slugs), `timezone` (e.g. `1`), `sort` (`relevant`, `recent`, `salaryDesc`, ...), `page` (1-based; stop on empty page; dedupe on `guid`).
- Auth: none needed. A free API key raises limits to 60 req/min and 10,000/day. Without a key, all callers share one public allowance. 429 on excess.
- Terms: link back to the Himalayas URL and name Himalayas. Do not submit jobs to other boards. Apply via `applicationLink`. No CORS: call from a backend. Data refreshes daily, so cache.
- Remote flag: all remote. `locationRestrictions` lists allowed countries (empty = worldwide).

| Need | Field |
| --- | --- |
| id | `guid` (the Himalayas job URL) |
| title | `title` |
| company | `companyName` (`companySlug`) |
| location | `locationRestrictions[]`, `timezoneRestrictions[]` |
| remote | implied (all) |
| salary | `minSalary`, `maxSalary`, `currency`, `salaryPeriod` |
| url | `guid` (link-back), `applicationLink` |
| posted | `pubDate` (Unix seconds), `expiryDate` |
| description | `description` (HTML), `excerpt` |

```json
{"comments":"...","updatedAt":1792350000,"offset":0,"limit":20,"totalCount":900,"nextCursor":"abc",
 "jobs":[{"title":"Senior Frontend Engineer","excerpt":"...","companyName":"Acme","companySlug":"acme",
  "companyLogo":"...","employmentType":"Full Time","minSalary":130000,"maxSalary":160000,"currency":"USD",
  "salaryPeriod":"annual","seniority":["Senior"],"locationRestrictions":["Germany"],"timezoneRestrictions":[1],
  "categories":["Frontend-Engineer"],"parentCategories":["Engineering"],"description":"<p>...</p>",
  "pubDate":1792300000,"expiryDate":1794900000,"applicationLink":"https://...","guid":"https://himalayas.app/companies/acme/jobs/senior-frontend-engineer"}]}
```

## 5. Hacker News "Ask HN: Who is hiring?" (Algolia)

- Docs: https://hn.algolia.com/api (JavaScript page; endpoints below live-probed).
- Find the latest thread (posted by user `whoishiring` on the 1st of each month):
  - `GET https://hn.algolia.com/api/v1/search_by_date?tags=story,author_whoishiring&hitsPerPage=5`
  - Pick the first hit whose `title` starts with `Ask HN: Who is hiring?`. Same query also returns "Who wants to be hired?" and "Freelancer?" threads.
  - Live result on 2026-10-18: `objectID` `49922569`, "Ask HN: Who is hiring? (October 2026)".
- Fetch comments, option A (one call, full tree): `GET https://hn.algolia.com/api/v1/items/49922569`. Top-level job posts are `children[]` (266 on probe date).
- Option B (flat, paged, searchable): `GET https://hn.algolia.com/api/v1/search?tags=comment,story_49922569&query=remote&hitsPerPage=100&page=0`. Filter top-level posts with `parent_id == story_id`.
- Auth: none. Rate limit: no official number found on the docs page. Third-party clients assume ~10,000 req/hour/IP. One call per month per thread is enough.
- Fields: posts are free text. By convention the first line is `Company | Role | Location | REMOTE/ONSITE | Salary | URL`. Parse that line.

| Need | Field (items API) | Field (search API) |
| --- | --- | --- |
| id | `id` | `objectID` |
| title / company / location / remote / salary | parse first line of `text` | parse `comment_text` |
| url | `https://news.ycombinator.com/item?id={id}` | same |
| posted | `created_at`, `created_at_i` | `created_at`, `created_at_i` |
| description | `text` (HTML) | `comment_text` (HTML) |

```json
{"id":49922569,"type":"story","author":"whoishiring","title":"Ask HN: Who is hiring? (October 2026)",
 "created_at":"2026-10-01T15:02:07.000Z","created_at_i":1790866927,"parent_id":null,"story_id":49922569,
 "children":[{"id":49922600,"type":"comment","author":"someone","parent_id":49922569,"story_id":49922569,
  "created_at":"2026-10-01T15:05:00.000Z","text":"Acme | Senior Engineer | Example City or REMOTE | $150k-$180k | https://acme.example.com/jobs<p>...","children":[]}]}
```

## 6. We Work Remotely RSS

- Feed list page: https://weworkremotely.com/remote-job-rss-feed (returns 403 to bots; feed URLs below live-probed 200).
- Feeds:
  - All jobs: `https://weworkremotely.com/remote-jobs.rss`
  - `https://weworkremotely.com/categories/remote-programming-jobs.rss`
  - `https://weworkremotely.com/categories/remote-full-stack-programming-jobs.rss`
  - `https://weworkremotely.com/categories/remote-back-end-programming-jobs.rss`
  - `https://weworkremotely.com/categories/remote-front-end-programming-jobs.rss`
  - `https://weworkremotely.com/categories/remote-devops-sysadmin-jobs.rss`
- Auth: none. Channel `<ttl>` is 60 minutes, so poll at most hourly.
- Terms: no API terms found. Link to the job `link` and name WWR as a courtesy.
- Item tags (live-probed): `title`, `region`, `country`, `state`, `skills`, `category`, `type`, `description`, `pubDate`, `guid`, `link`.

| Need | Field |
| --- | --- |
| id | `guid` |
| title / company | `title` = `"Company: Job title"` (split on first `: `) |
| location | `region` (e.g. "Anywhere in the World"), `country`, `state` |
| remote | implied (all) |
| salary | none (sometimes inside `description`) |
| url | `link` |
| posted | `pubDate` (RFC 822) |
| description | `description` (HTML, entity-escaped) |

```xml
<item>
  <title>Acme: Staff Software Engineer</title>
  <region>Anywhere in the World</region><country></country><state></state><skills></skills>
  <category>Back-End Programming</category><type>Full-Time</type>
  <description>&lt;p&gt;...&lt;/p&gt;</description>
  <pubDate>Fri, 17 Oct 2026 12:00:00 +0000</pubDate>
  <guid>https://weworkremotely.com/remote-jobs/acme-staff-software-engineer</guid>
  <link>https://weworkremotely.com/remote-jobs/acme-staff-software-engineer</link>
</item>
```

## 7. LinkedIn

- **No public job-search API for individuals.** The Job Posting API is partner-only and "not accepting new partnerships". https://learn.microsoft.com/en-us/linkedin/talent/job-postings/api/overview
- User Agreement bans scripts, robots, crawlers, or plugins that "scrape or copy the Services". https://www.linkedin.com/legal/user-agreement (section 8.2). careerbrew must not scrape LinkedIn or reuse session cookies.
- Allowed path 1: **your own data export.** https://www.linkedin.com/help/linkedin/answer/a1339364 , file contents: https://www.linkedin.com/help/linkedin/answer/50191
  - Desktop: Me → Settings & Privacy → Data privacy → Download your data → Request archive.
  - Some files arrive in ~10 min. The full archive (Connections, Messages) arrives within 24–48 h. Link valid 72 h.
  - LinkedIn does not publish column headers. Headers below are the ones commonly seen in real exports. **Parse by header name, case-insensitive. Do not hard-code order.**

| File in the ZIP | Contents (official) | Typical header row |
| --- | --- | --- |
| `Profile.csv` | name, address, birth date, zip, headline, summary | `First Name, Last Name, Maiden Name, Address, Birth Date, Headline, Summary, Industry, Zip Code, Geo Location, Twitter Handles, Websites, Instant Messengers` |
| `Positions.csv` | company, title, description, location, dates | `Company Name, Title, Description, Location, Started On, Finished On` |
| `Skills.csv` | skills on profile | `Name` |
| `Connections.csv` | 1st-degree name, URL, email (if shared), company, position, date | `First Name, Last Name, URL, Email Address, Company, Position, Connected On` — **starts with ~3 "Notes:" lines before the header** |
| `Jobs/Job Applications.csv` (may be split `_1`, `_2`) | applied date, company, title, job URL | `Application Date, Contact Email, Contact Phone Number, Company Name, Job Title, Job Url, Resume Name, Question And Answers` |
| `Jobs/Saved Jobs.csv` | saved date, title, company, job URL | `Saved Date, Job Url, Job Title, Company Name` |
| `Jobs/Job Seeker Preferences.csv` | locations, industries, titles, company size, activity | varies |
| `SavedJobAlerts.csv` (name varies) | saved job search terms + dates | varies |

- Allowed path 2: **job alert emails** you already receive.
  - From: `LinkedIn Job Alerts <jobalerts-noreply@linkedin.com>` (sender address confirmed by third-party guides).
  - Subject: not officially documented. Commonly `"<search term>": <Company> - <Title> and more` or `<N> new jobs for "<search term>"`. Match on the sender, not the subject.
  - Other senders: `jobs-noreply@linkedin.com` (application confirmations, "Your application was sent to ..."), `messages-noreply@linkedin.com`, `notifications-noreply@linkedin.com`.

## 8. Other boards (one line each)

- **Wellfound (ex AngelList Talent):** no public jobs API. The old AngelList API is gone. Scrapers only.
- **Welcome to the Jungle / Otta:** Otta is part of Welcome to the Jungle. No public jobs API found.
- **Indeed:** Publisher job-search API is retired. Current APIs are partner-gated (ATS / employers).
- **Glassdoor:** no public API for job or salary search. Only unofficial scrapers, against their terms.

---

## 9. Microsoft Graph (Outlook / Microsoft 365)

- Docs: messages https://learn.microsoft.com/en-us/graph/api/user-list-messages?view=graph-rest-1.0 , calendarView https://learn.microsoft.com/en-us/graph/api/user-list-calendarview?view=graph-rest-1.0 , device code https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code
- **Messages:** `GET https://graph.microsoft.com/v1.0/me/messages`
  - Example: `?$select=id,internetMessageId,from,subject,receivedDateTime&$top=50&$orderby=receivedDateTime desc&$filter=receivedDateTime ge 2026-10-11T00:00:00Z`
  - `$top`: 1–1000, default 10. Next page: follow `@odata.nextLink` as-is.
  - With `$filter` + `$orderby`: the `$orderby` property must also appear first in `$filter`, or you get `InefficientFilter`.
  - Folder: `/me/mailFolders/inbox/messages`.
  - Least-privileged delegated scope: `Mail.ReadBasic` (no body). `Mail.Read` if the body is needed.
  - Fields: `id`, `internetMessageId`, `subject`, `from.emailAddress.{name,address}`, `sender`, `receivedDateTime`, `bodyPreview`, `webLink`.
- **Calendar for a day:** `GET https://graph.microsoft.com/v1.0/me/calendarView?startDateTime=2026-10-18T00:00:00Z&endDateTime=2026-10-19T00:00:00Z&$select=subject,start,end,location,organizer,isAllDay,onlineMeeting&$orderby=start/dateTime`
  - Both params required, ISO 8601. No offset = UTC.
  - Header `Prefer: outlook.timezone="Etc/GMT+4"` sets the zone of `start`/`end` in the response.
  - Expands recurring events into single occurrences.
  - Least-privileged delegated scope: `Calendars.ReadBasic`. `Calendars.Read` for full details.
  - Fields: `id`, `subject`, `start.{dateTime,timeZone}`, `end.{...}`, `location.displayName`, `organizer.emailAddress`, `isAllDay`, `iCalUId`.
- **Device code flow (public client, no secret):**
  - App registration: Entra admin center → App registrations. Enable "Allow public client flows".
  - `POST https://login.microsoftonline.com/{tenant}/oauth2/v2.0/devicecode` with form `client_id=...&scope=offline_access Mail.ReadBasic Calendars.ReadBasic`
    - `{tenant}`: `common` (any account), `consumers` (outlook.com personal), `organizations` (work/school), or a tenant id.
    - Response: `device_code`, `user_code`, `verification_uri`, `expires_in` (900 s default), `interval`, `message`.
  - Poll `POST https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token` with `grant_type=urn:ietf:params:oauth:grant-type:device_code&client_id=...&device_code=...` every `interval` seconds.
    - Errors: `authorization_pending` (keep polling), `authorization_declined`, `bad_verification_code`, `expired_token` (stop).
    - Success: `access_token`, `refresh_token` (only with `offline_access`), `expires_in`, `scope`.
  - Work accounts: a tenant admin may block user consent. Corporate machines may need admin approval.

```json
{"@odata.nextLink":"https://graph.microsoft.com/v1.0/me/messages?$skip=50...",
 "value":[{"id":"AAMkAG...","internetMessageId":"<abc@mail.example.com>","subject":"Interview invite",
  "from":{"emailAddress":{"name":"Recruiter","address":"recruiter@example.com"}},
  "receivedDateTime":"2026-10-17T14:03:00Z"}]}
```

## 10. Fastmail (JMAP + CalDAV)

- Docs: https://www.fastmail.com/dev/ , https://www.fastmail.com/for-developers/integrating-with-fastmail/ , JMAP Mail RFC 8621 https://www.rfc-editor.org/rfc/rfc8621.html , servers https://www.fastmail.help/hc/en-us/articles/1500000278342-Server-names-and-ports
- Session: `GET https://api.fastmail.com/jmap/session` with `Authorization: Bearer <API token>`.
  - Token: Settings → Privacy & Security → Manage API tokens. Choose read-only + Mail scope when creating it.
  - Fastmail says API tokens are for personal apps. Apps for other people must use OAuth.
  - Session response gives `apiUrl` (POST target) and `primaryAccounts["urn:ietf:params:jmap:mail"]` (the `accountId`).
- Find the Inbox id: `["Mailbox/get", {"accountId": A, "properties": ["id","name","role"]}, "m"]`, then pick `role == "inbox"`.
- List recent message metadata (POST to `apiUrl`):

```json
{"using":["urn:ietf:params:jmap:core","urn:ietf:params:jmap:mail"],
 "methodCalls":[
  ["Email/query",{"accountId":"u123","filter":{"inMailbox":"MBinbox","after":"2026-10-11T00:00:00Z"},
    "sort":[{"property":"receivedAt","isAscending":false}],"limit":50},"0"],
  ["Email/get",{"accountId":"u123","#ids":{"resultOf":"0","name":"Email/query","path":"/ids"},
    "properties":["id","messageId","from","subject","receivedAt","mailboxIds"]},"1"]]}
```

- Response: `methodResponses[1][1].list[]` with `id`, `messageId` (array of Message-ID strings), `from` (`[{name,email}]`), `subject`, `receivedAt`.
- Rate limits: no general numbers published.
- **Calendar:** no JMAP calendars yet. Options:
  - CalDAV: `https://caldav.fastmail.com/` with full address + **app password** (Settings → Privacy & Security → app passwords). Per-calendar URL: Settings → Calendars → Export.
  - ICS: Settings → Calendars → Edit & share → Publish → "Full event details" or "Free/busy". https://www.fastmail.help/hc/en-us/articles/360060590793
- IMAP fallback: `imap.fastmail.com:993` SSL/TLS, app password.

## 11. Proton Mail

- **No public REST API.** Proton publishes no developer mail API. The supported route is Proton Mail Bridge (local IMAP/SMTP). Unofficial tools (e.g. hydroxide) exist; avoid them.
- Bridge needs a **paid plan**. Desktop only (macOS, Windows, Linux). https://proton.me/support/imap-smtp-and-pop3-setup
- Defaults (https://proton.me/support/port-already-occupied-error): IMAP `127.0.0.1:1143`, SMTP `127.0.0.1:1025`. Change under Bridge → Settings → Advanced → Default ports.
- Security: STARTTLS by default with a self-signed local cert (can switch to SSL in Bridge settings). Not confirmed on the official page — read the mailbox details in the Bridge app.
- Username: your Proton address. Password: the **Bridge-generated password** shown in the Bridge app, not your Proton login.
- Read-only use: IMAP `SELECT INBOX` read-only (`EXAMINE`) + `FETCH 1:* (UID ENVELOPE INTERNALDATE)` gives from, subject, date, Message-ID.
- Calendar: Proton has no CalDAV. Use a shared ICS link (section 13).

## 12. ICS feeds (RFC 5545)

- Spec: https://www.rfc-editor.org/rfc/rfc5545
- Fields to read in each `VEVENT`:
  - `UID`: stable id.
  - `DTSTART` / `DTEND`: start / end. Forms: `DTSTART:20261018T140000Z` (UTC), `DTSTART;TZID=Etc/GMT+4:20261018T100000` (local + `VTIMEZONE`), `DTSTART;VALUE=DATE:20261018` (all-day; `DTEND` is exclusive). `DURATION` may replace `DTEND`.
  - `SUMMARY`: title. Also `LOCATION`, `DESCRIPTION`, `ORGANIZER`, `STATUS`.
  - `RRULE`: recurrence, e.g. `FREQ=WEEKLY;BYDAY=MO,WE;UNTIL=20261231T000000Z`. Apply `EXDATE` (skipped dates) and `RECURRENCE-ID` overrides. Use a library (Python `icalendar` + `recurring-ical-events`) for "events on day X".
- Private links (anyone with the link can read; store as secrets, never commit):
  - **Google:** Settings → [calendar] → Integrate calendar → "Secret address in iCal format". Reset invalidates it. Admins can hide it on work accounts. https://support.google.com/calendar/answer/37648
  - **Proton (paid):** calendar.proton.me → Settings → Calendars → Share with anyone → Create link → Full view (titles, details) or Limited view (busy only). Up to 5 links/calendar. Updates can lag up to 8 h. https://proton.me/support/share-calendar-via-link
  - **Fastmail:** Settings → Calendars → Edit & share → Publish → Full event details or Free/busy. https://www.fastmail.help/hc/en-us/articles/360060590793
  - **iCloud:** iCloud.com or device calendar settings → turn on "Public Calendar" → `webcal://pXX-caldav.icloud.com/published/2/...` link. Swap `webcal://` for `https://` to fetch. Exposes full details. Alternative: CalDAV `https://caldav.icloud.com/` with an app-specific password.

```text
BEGIN:VEVENT
UID:abc123@example.com
DTSTART;TZID=Etc/GMT+4:20261020T100000
DTEND;TZID=Etc/GMT+4:20261020T110000
SUMMARY:Interview - Acme
RRULE:FREQ=WEEKLY;COUNT=1
END:VEVENT
```
