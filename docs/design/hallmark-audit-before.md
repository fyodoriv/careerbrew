# Hallmark audit of web/: before

The Hallmark audit of `web/` on the unchanged base (`origin/main` 5e2aab2), 2026-10-27. Task `hallmark-design`.

**How it ran.** `/hallmark audit web/` in app scope (`sloplint.mjs src index.html --scope app --genre modern-minimal --render --before`), on the seeded e2e server, for eight views: Today, Dashboard, Week, Job hunt, Practice, Library, Setup, Review. The render tier opens each view at 320 to 1920 px. The render tier waited for `networkidle2`: the app's live poll never lets the page reach `networkidle0`, which the stock script waits for.

**The read.**
1. What it is for: one owner works through the day (the next step, the PM update, the agenda), the job hunt and practice.
2. What works and stays: the sidebar shell, the one agenda, the status-tinted tables, the recall card, and the comfort budget (44 px targets, 14 px text, no sideways scroll).
3. What reads as generated: a stock indigo accent with accent-coloured links, spacing off any scale (83 values), coloured side stripes on cards, a tracked-caps "NEXT STEP" label, dot-joined meta strings, no `:active` state, disabled shown by opacity alone.
4. What is broken: field edges at 1.2 to 1.5:1 against their surface (gate 40), and Practice scrolls sideways by 3 px at 320 px (gate 34).

**Baseline (the task's measurement):** 26 Floor (FAIL) and 922 Reflex findings. 834 of the Reflex findings are one gate (49b, a long link label that wraps on a phone), and 83 are spacing off the 4 px scale (24b).


## Counts

| Grade | Count |
|---|---|
| FAIL | 26 |
| REFLEX | 922 |
| WARN | 20 |

| Grade | Gate | Count |
|---|---|---|
| FAIL | 19 | 4 |
| FAIL | 24a | 9 |
| FAIL | 34 | 1 |
| FAIL | 39a | 1 |
| FAIL | 40 | 3 |
| FAIL | 48 | 1 |
| FAIL | 63 | 6 |
| FAIL | 73 | 1 |
| REFLEX | 24b | 83 |
| REFLEX | 25 | 1 |
| REFLEX | 26b | 1 |
| REFLEX | 38iii | 1 |
| REFLEX | 49b | 834 |
| REFLEX | 72 | 2 |
| WARN | 3 | 5 |
| WARN | 41 | 3 |
| WARN | 70 | 2 |
| WARN | F1 | 2 |
| WARN | F19 | 4 |
| WARN | F23 | 1 |
| WARN | F26 | 2 |
| WARN | F3 | 1 |

## Findings

- `FAIL 19` src/actions.tsx:20 dash "—" in copy | fix: A comma, a colon, a full stop or parentheses; never a dash
- `FAIL 19` src/pager.tsx:87 dash "–" in copy | fix: A comma, a colon, a full stop or parentheses; never a dash
- `FAIL 19` src/teach.tsx:372 dash "–" in copy | fix: A comma, a colon, a full stop or parentheses; never a dash
- `FAIL 19` src/views.tsx:140 dash "–" in copy | fix: A comma, a colon, a full stop or parentheses; never a dash
- `FAIL 24a` src/styles.css:1 +1 more raw spacing lengths with a matching --space-* token in this file | fix: Use the tokens
- `FAIL 24a` src/styles.css:39 gap: 16px on .side where --space-4 exists | fix: Use var(--space-4)
- `FAIL 24a` src/styles.css:39 padding: 16px 10px on .side where --space-4 exists | fix: Use var(--space-4)
- `FAIL 24a` src/styles.css:66 padding: 14px 16px on .card where --space-4 exists | fix: Use var(--space-4)
- `FAIL 24a` src/styles.css:97 gap: 6px 16px on .props where --space-4 exists | fix: Use var(--space-4)
- `FAIL 24a` src/styles.css:262 margin: 0 0 16px on .reader h2 where --space-4 exists | fix: Use var(--space-4)
- `FAIL 24a` src/styles.css:267 margin: 16px 0 4px on .prose h3 where --space-4 exists | fix: Use var(--space-4)
- `FAIL 24a` src/styles.css:295 padding: 6px 16px on .side where --space-4 exists | fix: Use var(--space-4)
- `FAIL 24a` src/styles.css:302 padding: 16px 16px 48px on main where --space-4 exists | fix: Use var(--space-4)
- `FAIL 34` /#/practice [320px] horizontal scroll: scrollWidth 323 > viewport 320 | fix: overflow-x: clip on html and body; find the overflowing block
- `FAIL 39a` src/styles.css:138 disabled via opacity alone on button:disabled | fix: Three disabled channels: fill, ink and cursor, plus the disabled attribute
- `FAIL 40` /#/library [1440px] a field's edge is under 3:1 against what surrounds it: input 1.54:1 | fix: Draw the field in --color-muted, or fill it a step off the surface it sits on
- `FAIL 40` /#/today [1440px] a field's edge is under 3:1 against what surrounds it: textarea.pm-body 1.22:1, input 1.54:1, input 1.54:1 | fix: Draw the field in --color-muted, or fill it a step off the surface it sits on
- `FAIL 40` /#/week [1440px] a field's edge is under 3:1 against what surrounds it: select 1.54:1, input 1.54:1, input 1.54:1 | fix: Draw the field in --color-muted, or fill it a step off the surface it sits on
- `FAIL 48` src/styles.css:352 colour literal rgb(0 0 0 / 0.18) outside tokens (box-shadow on .toast) | fix: Lift the value into the token block
- `FAIL 63` src/notifications.tsx:85 middle-dot meta string "On: priority or more · quiet hours · at most pop-…" | fix: Line breaks, columns or hairlines; one dot in a byline at most
- `FAIL 63` src/reports.tsx:30 middle-dot meta string "· done · blocker · next" | fix: Line breaks, columns or hairlines; one dot in a byline at most
- `FAIL 63` src/teach.tsx:349 middle-dot meta string "drives today · · · min/day Edit Remove" | fix: Line breaks, columns or hairlines; one dot in a byline at most
- `FAIL 63` src/teach.tsx:350 middle-dot meta string "drives today · · · min/day" | fix: Line breaks, columns or hairlines; one dot in a byline at most
- `FAIL 63` src/views.tsx:948 middle-dot meta string "score · h · due" | fix: Line breaks, columns or hairlines; one dot in a byline at most
- `FAIL 63` src/views.tsx:1181 middle-dot meta string "Streak d · new a day · leeches" | fix: Line breaks, columns or hairlines; one dot in a byline at most
- `FAIL 73` /#/setup [1440px] text painted under another element: code over "Setting"; code over "Value"; td over "From" | fix: Give the text its own space, or put it above what covers it on a surface of its own
- `REFLEX 24b` src/styles.css:39 padding: 16px 10px (off the 4px scale) on .side | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:44 gap: 14px (off the 4px scale) on nav | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:47 padding: 5px 8px (off the 4px scale) on nav a | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:66 padding: 14px 16px (off the 4px scale) on .card | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:67 margin-bottom: 10px (off the 4px scale) on .card-head | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:75 margin-top: 10px (off the 4px scale) on .foot | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:78 margin: 0 0 10px (off the 4px scale) on .big-number | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:83 padding: 10px 12px (off the 4px scale) on pre.code | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:84 padding-left: 18px (off the 4px scale) on .list | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:91 gap: 6px 10px (off the 4px scale) on .chips | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:91 margin: 10px 0 (off the 4px scale) on .chips | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:97 gap: 6px 16px (off the 4px scale) on .props | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:102 padding: 0 6px (off the 4px scale) on .tag | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:108 padding-top: 10px (off the 4px scale) on .watched-project | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:111 margin-right: 6px (off the 4px scale) on .task-id | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:117 padding: 10px 12px (off the 4px scale) on .callout | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:120 padding: 8px 10px (off the 4px scale) on .note | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:123 padding: 12px 14px (off the 4px scale) on .skeleton | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:129 margin: 0 0 6px (off the 4px scale) on .updated | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:134 padding: 7px 12px (off the 4px scale) on input, select, button | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:142 gap: 6px (off the 4px scale) on .inline | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:145 gap: 6px (off the 4px scale) on .field | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:147 margin-bottom: 10px (off the 4px scale) on .search | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:148 padding: 0 5px (off the 4px scale) on kbd | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:156 padding: 7px 8px (off the 4px scale) on th, td | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:164 padding: 6px 0 (off the 4px scale) on .table-wrap tr | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:166 padding: 3px 8px 3px min(36%, 150px) (off the 4px scale) on .table-wrap td, .table-wrap td.num | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:176 gap: 6px (off the 4px scale) on .shares | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:180 margin-bottom: 10px (off the 4px scale) on .stats | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:181 padding: 10px 12px (off the 4px scale) on .stat | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:192 margin-right: 6px (off the 4px scale) on .dot | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:200 gap: 6px (off the 4px scale) on .agenda | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:201 gap: 10px (off the 4px scale) on .block | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:201 padding: 7px 10px (off the 4px scale) on .block | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:208 padding: 10px (off the 4px scale) on .day | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:212 padding: 6px 8px (off the 4px scale) on .day .block | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:217 padding: 0 0 14px 38px (off the 4px scale) on .phases li, .stepper li | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:223 padding-bottom: 18px (off the 4px scale) on .stepper li | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:231 padding: 8px 10px (off the 4px scale) on .tile | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:236 padding: 18px 20px (off the 4px scale) on .study | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:239 padding: 14px 0 2px (off the 4px scale) on .flashcard .answer | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:243 gap: 6px (off the 4px scale) on .grade | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:254 margin-top: 10px (off the 4px scale) on .tree | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:255 gap: 6px (off the 4px scale) on .tree h3 | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:266 margin: 20px 0 6px (off the 4px scale) on .prose h2 | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:268 margin: 0 0 10px (off the 4px scale) on .prose p, .prose ul, .prose ol | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:269 padding-left: 22px (off the 4px scale) on .prose ul, .prose ol | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:270 margin: 3px 0 (off the 4px scale) on .prose li | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:271 padding: 10px 12px (off the 4px scale) on .prose pre | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:273 margin: 0 0 10px (off the 4px scale) on .prose table | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:274 margin: 0 0 10px (off the 4px scale) on .prose blockquote | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:295 padding: 6px 16px (off the 4px scale) on .side | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:315 padding: 13px 0 (off the 4px scale) on main a:not(.button):not(.brand) | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:315 margin: -13px 0 (off the 4px scale) on main a:not(.button):not(.brand) | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:319 padding: 6px 8px (off the 4px scale) on main .tree a:not(.button), main .report-list a:not(.button) | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:322 margin-top: 14px (off the 4px scale) on .report | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:324 margin-top: 10px (off the 4px scale) on .explain .page | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:330 padding: 8px 10px (off the 4px scale) on textarea | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:331 gap: 10px (off the 4px scale) on .goal-form | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:341 padding: 8px 10px (off the 4px scale) on .pm-body | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:352 padding: 10px 14px (off the 4px scale) on .toast | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:354 padding: 6px 12px (off the 4px scale) on a.button | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:358 padding: 6px 10px (off the 4px scale) on .error | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:358 margin: 6px 0 (off the 4px scale) on .error | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:359 margin-right: 6px (off the 4px scale) on .working::before | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:373 gap: 6px (off the 4px scale) on .notification-head | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:375 margin: 0 0 6px (off the 4px scale) on .notification-title | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:379 margin: 0 0 6px (off the 4px scale) on .standup-talk | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:386 margin-top: 10px (off the 4px scale) on .pager | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:387 margin: 0 0 10px (off the 4px scale) on .pager-filter | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:388 padding: 2px 10px (off the 4px scale) on .chip | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:393 margin-bottom: 10px (off the 4px scale) on .fold > details[open] > summary | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:404 margin-bottom: 6px (off the 4px scale) on .settings-group[open] > summary | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:415 padding: 6px 0 (off the 4px scale) on .topics li | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:420 padding: 10px 0 (off the 4px scale) on .tree > summary, .settings-group > summary | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:430 margin: 0 0 10px (off the 4px scale) on .explainer | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:431 margin: 0 0 6px (off the 4px scale) on .maybe-done | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:432 margin: 6px 0 (off the 4px scale) on .confirm | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:433 margin: 0 0 6px (off the 4px scale) on .confirm > p | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:435 gap: 6px 12px (off the 4px scale) on .pager-tools | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:435 margin: 0 0 10px (off the 4px scale) on .pager-tools | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:437 gap: 6px (off the 4px scale) on .checklist | fix: Use the 4px spacing scale
- `REFLEX 24b` src/styles.css:443 padding: 10px 14px 0 (off the 4px scale) on .report-reader > .about | fix: Use the 4px spacing scale
- `REFLEX 25` /#/today [1440px] running prose set too wide: p.explainer about 104 characters a line | fix: Cap the measure at 45 to 75ch
- `REFLEX 26b` src/styles.css:1 no :active styling anywhere | fix: All eight states; guard: not a form control, no aria-disabled anywhere
- `REFLEX 38iii` src/styles.css:225 accented word inside a heading (.stepper h3 .tag) | fix: Set the whole head one way: a line break, a second line at a second size, or the whole head in the accent. Guard: the theme names the coloured word as its signature and it appears once on the page, in the hero
- `REFLEX 72` /#/today [1440px] text crowds the edge of its box: li.block bottom -12px; li.block bottom -12px | fix: Pad the box: max(8px, 0.5em) at the sides, max(4px, 0.3em) above and below
- `REFLEX 72` /#/week [1440px] text crowds the edge of its box: li.block bottom -13px; li.block bottom -13px; li.block bottom -13px | fix: Pad the box: max(8px, 0.5em) at the sides, max(4px, 0.3em) above and below
- `WARN 3` src/styles.css:195 side stripe border-left: 3px solid var(--accent) on .hero | fix: Hairline all round, none, or an accent square
- `WARN 3` src/styles.css:201 side stripe border-left: 3px solid on .block | fix: Hairline all round, none, or an accent square
- `WARN 3` src/styles.css:274 side stripe border-left: 3px solid var(--line-strong) on .prose blockquote | fix: Hairline all round, none, or an accent square
- `WARN 3` src/styles.css:332 side stripe border-left: 3px solid var(--accent) on .teacher-says | fix: Hairline all round, none, or an accent square
- `WARN 3` src/styles.css:381 side stripe border-left: 3px solid var(--accent) on .standup-row.mine | fix: Hairline all round, none, or an accent square
- `WARN 41` src/styles.css:66 dark surface (L 22%) on .card sets no text colour | fix: Swap text to paper in the same rule
- `WARN 41` src/styles.css:182 dark surface (L 19%) on .card .stat sets no text colour | fix: Swap text to paper in the same rule
- `WARN 41` src/styles.css:202 dark surface (L 29%) on .block-task sets no text colour | fix: Swap text to paper in the same rule
- `WARN 70` src/daylog.tsx:1 a script prints success ("Sent to ${r.sent.to}") with no request behind it | fix: No success state until the form posts somewhere; an empty data-endpoint and a handoff note
- `WARN 70` src/views.tsx:1 a script prints success ("careerbrew work done <id>") with no request behind it | fix: No success state until the form posts somewhere; an empty data-endpoint and a handoff note
- `WARN F1` src/daylog.tsx:111 straight quote in prose x1 (first: Set your PM's address:) | fix: Use typographic quotes: “ ” ’
- `WARN F1` src/views.tsx:374 straight quote in prose x3 (first: Today's plan) | fix: Use typographic quotes: “ ” ’
- `WARN F3` src/styles.css:53 no text-wrap: balance on any display head (10 head rule(s), e.g. .page-head h1) | fix: Add text-wrap: balance to h1/h2-level heads
- `WARN F19` /#/dashboard [1440px] 4 font weights in running use (400, 500, 600, 700) | fix: Three weights at most, display and body together, picked as extremes
- `WARN F19` /#/jobs [1440px] 4 font weights in running use (400, 500, 600, 700) | fix: Three weights at most, display and body together, picked as extremes
- `WARN F19` /#/practice [1440px] 4 font weights in running use (400, 500, 600, 700) | fix: Three weights at most, display and body together, picked as extremes
- `WARN F19` /#/today [1440px] 4 font weights in running use (400, 500, 600, 700) | fix: Three weights at most, display and body together, picked as extremes
- `WARN F23` index.html:1 the document lacks a meta description | fix: Name the language, title the page in its own words, and describe it in one sentence for link previews
- `WARN F26` /#/library [375px] h3 "Interview knowledge · Computer s" ends on a stub; h3 "Interview knowledge · Web develo" ends on a stub | fix: text-wrap: balance on heads and pretty on body copy, or rewrite the line; a <br> goes only between phrases and is checked at 375
- `WARN F26` /#/library [1280px] h3 "Interview knowledge · Business/p" ends on a stub; h3 "Interview knowledge · Computer s" ends on a stub; h3… | fix: text-wrap: balance on heads and pretty on body copy, or rewrite the line; a <br> goes only between phrases and is checked at 375

## Gate 49b (a link label wraps), grouped

One finding per long link at 320 px; the labels are the owner's titles.

- /#/dashboard: 1 links, for example: [320px] link "code.example.com/…/test_checkout" wraps to 2 lines (a) | fix: Shorten the label or set nowrap
- /#/jobs: 4 links, for example: [320px] link "Staff Software Engineer, Platfor" wraps to 2 lines (a) | fix: Shorten the label or set nowrap
- /#/library: 823 links, for example: [320px] link "How can AI solve customer proble" wraps to 2 lines (a) | fix: Shorten the label or set nowrap
- /#/today: 1 links, for example: [320px] link "code.example.com/…/test_checkout" wraps to 2 lines (a) | fix: Shorten the label or set nowrap
- /#/week: 5 links, for example: [320px] link "code.example.com/…/test_checkout" wraps to 2 lines (a) | fix: Shorten the label or set nowrap
