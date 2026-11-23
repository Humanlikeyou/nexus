# 0042: Every action carries through to completion (Taisun, 2026-11-19, in session)

His words (verbatim):
- "Go throught the enitre app. fix any buttons or path gaps in the app and make sure everyhting is working and check all the text especially in goals and nuero. some text may be overlapping and not adhering to the golden ratio fibinnocii rule"
- "fix this on the today page also "Crew approvals didn't load. The rest is current.""
- "Also the entangled waves on the goal page sould be completly randon but occasionaaly it should harmonize"
- "use other models were best fit"
- **"The app has a lot of individual features and interactive elements, but they aren't fully connected into a cohesive, functional workflow. I want every button, input, and action to trigger the appropriate next step, pass information between connected systems, and carry the user's intent all the way through to completion. Nothing should be a dead end or an isolated feature."**

What this means for the work:
1. **Every interactive element has a next step.**
   - Every button, input, link, row and card does something visible.
   - It leads to where the job continues: the screen, the item and the filled-in context. It never just lands on the top of a page.
   - It ends in a completion state the user can see: a confirmation, Undo, History, or the item leaving a list.
2. **Context carries.**
   - When one feature hands off to another, the user's intent and data go with it. Examples: a take's question opens chat pre-filled; a Needs-you row opens that exact item; a watchlist pick links to its source; a goal step done updates progress and the weekly review.
3. **No isolated features.** Each feature's outputs feed the features that should use them, through the existing backends.
4. **Text and layout:**
   - text on the φ type scale 11/13/17/21/27/34 (docs/design/golden-ratio.md), Fibonacci spacing
   - no overlapping or clipped text, with Goals and Neuro first
5. **Method:**
   - a whole-app crawler (layout and dead buttons) plus workflow maps per area
   - fixes in lanes by Sonnet agents where they fit, with review, gates and commits by the orchestrator
6. **Guard rails stay:**
   - no orders or sends without his confirm, approvals stay native, no new outbound host
   - "Needs wiring" only where a real data gap or gate exists, and that gap is listed
   - Taisun approves visuals from images
