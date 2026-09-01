# Bun-Inspired Project-Agnostic Design System

**Working name:** `Warm Technical`

**Design archetype:** Developer tooling + editorial print + terminal UI.

**Primary characteristics:**

* Warm cream instead of sterile white.
* Near-black typography instead of gray-heavy SaaS UI.
* One saturated playful accent.
* Heavy grotesk display typography.
* Monospace treated as an actual product surface.
* Technical evidence—benchmarks, output, code, APIs—is part of the visual design.
* Thin borders and tonal separation instead of floating shadow cards.
* Large editorial sections mixed with extremely dense developer information.
* Very little decorative chrome.
* Playfulness is concentrated in brand moments rather than spread across every control.

---

# 1. Design DNA

Bun’s strongest idea is **contrast between warmth and technical density**.

The foundation feels almost like printed packaging:

**cream paper → near-black ink → pink highlight → dark terminal**

That makes technical interfaces feel less corporate without making them childish.

The basic visual hierarchy should be:

```text
Warm page canvas
    ↓
Editorial typography
    ↓
White / light paper surfaces
    ↓
Hairline boundaries
    ↓
Near-black technical artifacts
    ↓
One strong accent
```

Do not build this style around gradients, glassmorphism, enormous glows, generic purple-blue SaaS effects, or dozens of card elevations.

---

# 2. Color System

The most useful Bun-like palette is:

| Token               |                Value | Usage                             |
| ------------------- | -------------------: | --------------------------------- |
| `canvas`            |            `#FBF0DF` | Main page background              |
| `canvas-soft`       |            `#FFF7E8` | Alternating page section          |
| `canvas-pressed`    |            `#F5E8D0` | Active/pressed warm surface       |
| `surface`           |            `#FFFFFF` | Cards, panels                     |
| `surface-soft`      |            `#FDFAF2` | Nested surface                    |
| `foreground`        |            `#15151B` | Default text                      |
| `foreground-strong` |            `#000000` | Maximum emphasis                  |
| `muted`             |            `#6B6B6B` | Descriptions                      |
| `faint`             |            `#9A9A9A` | Metadata                          |
| `brand`             |            `#F472B6` | Main playful accent               |
| `brand-hover`       |            `#DB2777` | Hover / link                      |
| `brand-soft`        |            `#FCE7F3` | Selected / highlighted background |
| `brand-faint`       |            `#FDF2F8` | Very subtle brand tint            |
| `accent`            |            `#FBBF24` | Secondary warm accent             |
| `accent-hover`      |            `#F59E0B` | Stronger accent                   |
| `accent-soft`       |            `#FEF3C7` | Highlight background              |
| `link`              |            `#DB2777` | Inline links                      |
| `link-hover`        |            `#9D174D` | Link hover                        |
| `border`            | `rgba(21,21,27,.10)` | Default border                    |
| `border-strong`     | `rgba(21,21,27,.20)` | Strong boundary                   |
| `border-subtle`     | `rgba(21,21,27,.05)` | Low-emphasis boundary             |
| `divider`           | `rgba(21,21,27,.08)` | Section/table divider             |
| `code-bg`           |            `#15151B` | Code/terminal surface             |
| `code-fg`           |            `#FBF0DF` | Code foreground                   |
| `code-muted`        |            `#A1A1AA` | Comments / secondary output       |

The warm cream / pink / near-black combination and low-shadow paper-like layering are recurring characteristics of Bun’s current visual treatment.

## Accent discipline

This matters enormously.

Use approximately:

```text
80% cream / neutral
15% black / dark surfaces
5% pink + yellow
```

Pink should mean:

* selection
* product personality
* interactive emphasis
* badges
* small highlighted phrases
* occasional featured surfaces

It should **not** cover half the website.

Yellow/gold should appear less frequently than pink.

Avoid adding cyan, violet, blue, green, orange, etc. as decorative colors.

Semantic colors may still exist for errors/success/warnings, but they are functional rather than decorative.

---

# 3. Typography

Bun uses a three-register typography approach: a chunky display face, neutral UI/body typography, and a strongly differentiated monospace environment. A recent visual audit identifies CoFo Sans, Inter, and Berkeley Mono respectively.

## Recommended project-agnostic stack

```css
--font-display:
  "CoFo Sans",
  "Inter Tight",
  "Arial Narrow",
  sans-serif;

--font-body:
  Inter,
  -apple-system,
  BlinkMacSystemFont,
  "Segoe UI",
  sans-serif;

--font-mono:
  "Berkeley Mono",
  "JetBrains Mono",
  "SFMono-Regular",
  Consolas,
  monospace;
```

If you don't license CoFo Sans/Berkeley Mono:

```text
Display: Inter Tight / Archivo
Body:    Inter
Mono:    JetBrains Mono
```

Do **not** try to make everything use the quirky display face.

---

## Type scale

| Role       | Desktop | Weight | Line height | Tracking |
| ---------- | ------: | -----: | ----------: | -------: |
| Hero XL    |    96px |    800 |         .95 |      -4% |
| Hero       |    80px |    800 |           1 |      -3% |
| Display    |    64px |    700 |        1.05 |    -2.5% |
| H1         |    52px |    700 |        1.10 |      -2% |
| H2         |    40px |    700 |        1.15 |    -1.8% |
| H3         |    30px |    700 |        1.20 |    -1.2% |
| H4         |    24px |    600 |        1.30 |     -.8% |
| H5         |    20px |    600 |        1.35 |     -.5% |
| Lead       |    19px |    400 |        1.60 |     -.5% |
| Body       |    17px |    400 |        1.60 |   normal |
| Small      |    15px |    400 |        1.55 |   normal |
| Caption    |    13px |    500 |        1.50 |      +1% |
| Eyebrow    |    13px |    600 |        1.40 |      +8% |
| Mono       |    14px |    400 |        1.60 |   normal |
| Mono Small |    12px |    400 |        1.55 |   normal |

### Rules

Marketing headings should be **heavy and tightly tracked**.

Documentation headings should become dramatically calmer:

```text
Marketing H1: 64–96
Documentation H1: 40–52
Article H1: 48–64
API symbol H1: 32–40
```

Body text should never inherit the display font.

Numbers in benchmarks should use tabular figures.

---

# 4. Spacing

Use a 4px underlying grid.

```text
2
4
8
12
16
20
24
32
40
48
64
80
96
128
160
```

Recommended semantic tokens:

```css
--space-1: 4px;
--space-2: 8px;
--space-3: 12px;
--space-4: 16px;
--space-5: 20px;
--space-6: 24px;
--space-8: 32px;
--space-10: 40px;
--space-12: 48px;
--space-16: 64px;
--space-20: 80px;
--space-24: 96px;
--space-32: 128px;
--space-40: 160px;
```

Bun-like marketing sections should breathe:

```text
Section Y padding: 80–128px
Card padding:      20–32px
Hero padding top:  80–120px
Docs blocks:       24–48px vertical
```

---

# 5. Layout System

Base measurements:

```text
Global max width:       1200px
Article/prose width:    680–720px
Header height:          64px
Desktop gutter:         24px
Mobile gutter:          16px
Marketing grid:         12 columns
```

The 1200px page width, ~680px prose column, 64px header and 24px gutter are consistent with the measured Bun-inspired system.

## Marketing grid

```text
|------------------- 1200 -------------------|

|  hero copy                | demo           |
|  5–6 columns              | 6–7 columns    |

| card | card | card | card |

|------------ large proof module ------------|
```

Break out of the prose width aggressively for:

* code demos
* terminal demos
* benchmark charts
* comparison tables
* testimonials
* product matrices

---

# 6. Border & Depth System

Radius:

```text
2px   micro
4px   tiny
6px   control
8px   button
12px  card/code
16px  major panel
999px pill
```

Do **not** make everything `rounded-2xl`.

Default card:

```css
background: #fff;
border: 1px solid rgba(21, 21, 27, .10);
border-radius: 12px;
box-shadow: none;
```

Depth primarily comes from:

```text
cream page
→ white card
→ border
→ slightly darker nested surface
```

Use shadows almost exclusively for:

* dropdowns
* command palettes
* floating overlays
* unusually elevated interactive elements

---

# 7. Global Header

Bun maintains the same primary navigation vocabulary across the major product surfaces: Home, Docs, Guides, Reference and Blog, with install as a persistent high-value action.

## Desktop

```text
┌──────────────────────────────────────────────────────────────┐
│ Brand     Docs   Guides   Reference   Blog       Install →  │
└──────────────────────────────────────────────────────────────┘
```

Specifications:

```text
height: 64px
position: sticky
background: rgba(251,240,223,.80–.92)
backdrop-filter: blur(8px)
border-bottom: 1px solid divider
```

Links:

```text
15px / 500
minimal padding
no pill behind inactive item
```

The header should feel like typography sitting on paper rather than a toolbar.

---

# 8. Announcement Strip

Optional strip above navbar.

```text
[ NEW ] Product v2.0 released →
```

Keep it one line.

Do not use a giant marketing ribbon.

Good:

```text
NEW  v2.0 released →
```

Bad:

```text
🎉 SUPER EXCITING ANNOUNCEMENT CLICK HERE 🚀
```

---

# 9. Buttons

## Primary

```text
background: near-black
foreground: cream
height: 44–48px
radius: 8px
font-weight: 650–700
padding-x: 20–24px
```

Hover:

```text
background → black
translateY(-1px)
```

## Brand

```text
background: pink
foreground: near-black
```

Use much less frequently.

## Secondary

```text
transparent
1px strong border
near-black text
```

## Text link

```text
pink/dark pink
arrow optional
underline only on hover
```

Primary actions should usually stay black, allowing the pink accent to remain special. This is one of the better lessons to take from Bun rather than turning the entire UI pink.

---

# 10. Terminal Surface

This is one of the most important components.

```text
┌────────────────────────────────────────────────┐
│ ~/project                       terminal       │
├────────────────────────────────────────────────┤
│ $ package install                               │
│                                                │
│ + package-a                                    │
│ + package-b                                    │
│                                                │
│ 431 packages installed [0.48s]                 │
└────────────────────────────────────────────────┘
```

Styles:

```text
background: #15151B
foreground: #FBF0DF
radius: 12px
padding: 20–24px
font: mono
size: 13–14px
line-height: 1.6
```

Header can be:

* filename
* working directory
* language
* copy action
* status
* sequence number

Never make code merely `<pre>` text floating in a generic gray rectangle.

It is a **product artifact**.

---

# 11. Code Block

Variants:

### Code

```text
filename.ts                      Copy

const server = createServer({
  port: 3000
});
```

### Terminal

```text
terminal                         Copy

$ package install
```

### Output

```text
output

Installed 532 packages [0.42s]
```

### Diff

```text
- npm install
+ product install
```

### Multi-tab

```text
JavaScript   TypeScript   CLI
──────────────────────────────
...
```

Use a quiet filename bar.

Do not imitate macOS traffic-light dots unless an actual window metaphor is important.

---

# 12. Inline Code

```text
background: subtle near-black tint
border: 1px solid subtle
font: mono
font-size: .88em
radius: 4px
padding: 1px 5px
```

On cream backgrounds:

```text
background: rgba(21,21,27,.05)
```

---

# 13. Tool / Product Cards

Bun’s homepage explicitly organizes its runtime, package manager, test runner and bundler as related but independently adoptable tools.

Generalize that pattern.

```text
┌──────────────────────────┐
│ Runtime                  │
│                          │
│ Executes your application│
│ with native APIs...      │
│                          │
│ $ product run app.ts     │
└──────────────────────────┘
```

Desktop:

```text
4 columns
```

Tablet:

```text
2 columns
```

Mobile:

```text
1 column
```

Cards should communicate:

1. category
2. positioning
3. short explanation
4. command/API/example

No icon is required.

---

# 14. Benchmark / Evidence Module

A critical Bun pattern is turning performance claims into structured benchmark modules, rather than simply writing “30× faster.” The homepage contains installation, HTTP, SQL, WebSocket, memory and package-manager comparisons with methodology links and numeric context.

General component:

```text
Installing dependencies

warm cache · lower is better

Product                   0.21s ━━━
Competitor A               1.76s ━━━━━━━━━━━
Competitor B               1.92s ━━━━━━━━━━━━
Competitor C               4.45s ━━━━━━━━━━━━━━━━━━━━━

12 MB peak memory

hardware · methodology · median of 3     reproduce →
```

Use:

```text
mono for numbers
body font for labels
strong black for winner
faded neutrals for competitors
pink sparingly for emphasis
```

Never use giant rainbow dashboards.

---

# 15. Statistics

Good:

```text
7×
faster warm installs

1,400+
additional tests passing

−18 MB
smaller binary
```

Structure:

```text
large figure
tiny descriptor
optional supporting line
```

Do not turn stats into glossy SaaS cards.

They can simply sit in a bordered grid.

---

# 16. Tabs / Segmented Controls

Examples:

```text
macOS & Linux    Windows
```

```text
Everything    Articles    Releases
```

```text
HTTP   WebSocket   SQL   Redis   Storage
```

Selected state:

```text
foreground: near-black
background: white / brand-soft
border: subtle
```

Inactive:

```text
transparent
muted foreground
```

Compact.

---

# 17. Feature Explorer

Bun’s homepage uses categories to expose different built-in APIs and then gives users real code for the selected capability.

This is much better than generic icon cards.

Pattern:

```text
HTTP  WebSockets  Database  Cache  Storage  Shell

[ short explanation ]

[ CODE EXAMPLE                                  ]
[                                               ]
[                                               ]
```

Use this for:

* SDK products
* databases
* developer platforms
* frameworks
* APIs
* infrastructure products

---

# 18. Workflow Stepper

Another useful Bun pattern is the “one workflow” narrative: install → develop → run → test → ship, paired with real commands/output.

General structure:

```text
01  Install
02  Develop
03  Run
04  Test
05  Ship
```

Selected item:

```text
strong text
small index
short command
short description
```

Adjacent panel:

```text
interactive terminal/demo
```

This gives the page a product-tour feeling without requiring screenshots of a dashboard.

---

# 19. Comparison Table

Use for feature matrices.

```text
                    Product     A      B
Runtime             ✓           ✓      ✓
TypeScript          ✓           —      ✓
Built-in database   ✓           —      —
Test runner         ✓           —      ✓
```

Style:

```text
no giant card
hairline row dividers
sticky first column when needed
mono/tnum numbers
bold winning product column
```

Bun ends the homepage with a large feature comparison rather than wrapping every capability in a separate sales card.

---

# 20. Testimonial / Production Story

Bun’s homepage uses recognizable production use-cases with person/company context and a concrete engineering claim rather than generic praise.

Generalize it as:

```text
[ avatar ]

Person Name
Role · Company

“Concrete technical result or architecture.”

Supporting explanation...

Relevant documentation →
```

Avoid:

> “Amazing product! We love it!”

Prefer:

> “We replaced three services with one binary and reduced cold-start overhead.”

Technical proof is the brand voice.

---

# 21. Docs Shell

The current docs expose a large persistent information hierarchy for runtime, package manager, bundler, test runner and other categories; content pages add breadcrumbs, headings, code examples, next/previous navigation, GitHub editing and Markdown access.

Desktop:

```text
┌──────────────────────────────────────────────────────────────┐
│ Global navigation                                            │
├──────────────┬───────────────────────────┬───────────────────┤
│ Docs sidebar │ Main document             │ On this page      │
│              │                           │                   │
│ Category     │ Breadcrumb                │ Installation      │
│   Page       │ H1                        │ Configuration     │
│   Page       │ Lead                      │ Deployment        │
│ Category     │                           │                   │
│   Page       │ Code                      │                   │
│              │                           │                   │
└──────────────┴───────────────────────────┴───────────────────┘
```

Recommended widths:

```text
left sidebar: 240–280px
content:      minmax(0, 720px)
right TOC:    180–220px
gaps:         32–48px
```

Page may therefore exceed marketing's 1200px maximum.

Use approximately:

```text
docs max-width: 1440–1520px
```

---

# 22. Docs Sidebar

Hierarchy:

```text
Runtime

GET STARTED
Welcome
Installation
Quickstart
TypeScript

CORE RUNTIME
Runtime
Watch Mode
Debugging

HTTP SERVER
Server
Routing
Cookies
TLS
```

Rules:

```text
Category label:
11–12px
uppercase or semibold
muted

Link:
14–15px
line-height ~28px

Active:
near-black text
subtle warm/pink background
optional 2px indicator
```

Keep the density.

Do not redesign the docs sidebar into enormous 44px menu items.

---

# 23. Docs Breadcrumb

```text
Runtime / HTTP Server / Server
```

Very low visual emphasis.

Use:

```text
13px
muted
```

The page title—not the breadcrumb—is the anchor.

---

# 24. Documentation Content

Preferred rhythm:

```text
H1

intro paragraph

code

body

────────────

H2

body

code

H3

body
```

Sections on Bun docs are frequently separated with horizontal rules, making long technical pages easier to scan.

Use the divider aggressively:

```css
border-top: 1px solid var(--divider);
margin-block: 48px;
```

---

# 25. Callouts

Variants:

```text
NOTE
TIP
WARNING
EXPERIMENTAL
DEPRECATED
```

Avoid icon-heavy colorful boxes.

Example:

```text
┌─────────────────────────────────────────┐
│ Experimental                            │
│ This API may change before v2.0.        │
└─────────────────────────────────────────┘
```

Background stays warm.

Use semantic border/text colors, not full saturation.

---

# 26. Page Navigation

At documentation bottom:

```text
← Previous                                  Next →
```

Below it:

```text
Edit this page on GitHub
View as Markdown
```

Bun uses both next/previous navigation and source-oriented actions for technical documentation.

This makes the docs feel like a living technical artifact.

---

# 27. Documentation Landing Page

Bun’s docs homepage does not begin with a huge marketing hero. It quickly introduces the four major tool areas, then gives users obvious starting points.

Template:

```text
Breadcrumb

# Documentation

One-sentence explanation.

┌────────────┐ ┌────────────┐
│ Runtime    │ │ Packages   │
│ ...        │ │ ...        │
└────────────┘ └────────────┘

┌────────────┐ ┌────────────┐
│ Testing    │ │ Building   │
└────────────┘ └────────────┘

────────────

## Get started

Installation →
Quickstart →

────────────

## What is Product?
...
```

Docs landing pages should optimize navigation, not conversion.

---

# 28. Guides Index

Bun separates **conceptual documentation** from **task-driven guides**, and its guide index groups many concrete tasks under categories such as deployment, ecosystem, networking, utilities, binary data and testing.

Template:

```text
# Guides
Code samples and walkthroughs for common tasks.

## Featured
[ guide ] [ guide ] [ guide ]

### Deployment

Deploy on Provider A
Deploy on Provider B
Deploy on Provider C

### Frameworks

Use Framework A
Use Framework B
Use Framework C

### Files

Read a file
Write a file
Stream a file
...
```

This should look closer to an **index/catalog** than a marketing card grid.

---

# 29. Guide Article

Bun’s individual guide pages are intentionally short and procedural: title, short context, code, explanatory step, terminal command, another step, then related navigation.

Template:

```text
Guides / Ecosystem

# Build an HTTP server

Short introductory sentence.

server.ts
┌──────────────────────────┐
│ code                     │
└──────────────────────────┘

────────────

Install dependencies:

terminal
┌──────────────────────────┐
│ $ package install        │
└──────────────────────────┘

────────────

Run the project:

terminal
┌──────────────────────────┐
│ $ package dev            │
└──────────────────────────┘

Previous ←                     → Next
```

No massive introduction.

Guides answer **how**.

Docs answer **what / why / configuration**.

---

# 30. Reference Index

Bun’s API reference is generated from its TypeScript definitions and organizes modules and symbols rather than explanatory tutorials.

Template:

```text
Reference

# Every API, from the types

Generated from the project's type definitions.

[ Browse symbols ]

## Modules

┌─────────────────────────────────────────┐
│ Core                                    │
│ Runtime APIs: HTTP, database, files... │
└─────────────────────────────────────────┘

Core
Testing
Storage
Globals
...
```

The key action is not “Get Started.”

It is:

```text
Browse symbols
```

---

# 31. Reference Symbol Browser

Command-palette style:

```text
┌──────────────────────────────────────────┐
│ Search symbols…                     ⌘K  │
├──────────────────────────────────────────┤
│ F  createServer                          │
│ C  Database                              │
│ I  ServerOptions                         │
│ T  RequestHandler                        │
└──────────────────────────────────────────┘
```

Symbol kind markers:

```text
F Function
C Class
I Interface
T Type
V Variable
N Namespace
P Property
M Method
```

Use tiny colored/muted markers.

This is one place where controlled multi-color syntax taxonomy is acceptable.

---

# 32. API Symbol Page

Current Bun reference pages present a breadcrumb path, symbol kind, type signature, documentation, examples and property/member lists.

Template:

```text
Modules / Core / Server / Options

interface

# ServerOptions<T>

port?: number

The port to listen on.

────────────

hostname?: string

Hostname used by the server.

────────────

error?: (...) => Response

Called when...
```

Type signatures use mono.

Descriptions remain body typography.

Important:

**Do not put the whole API reference inside a dark code block.**

Code is dark.

API documentation remains paper-like.

---

# 33. Search

Search is central to a developer system.

Trigger:

```text
Search docs…                /
```

Overlay:

```text
┌──────────────────────────────────────────┐
│ Search documentation...                 │
├──────────────────────────────────────────┤
│ Server                                  │
│ HTTP / Runtime                          │
│                                         │
│ WebSockets                              │
│ Networking                              │
└──────────────────────────────────────────┘
```

Desktop shortcut:

```text
/
```

or

```text
⌘K
```

Reference can have a more specialized symbol search.

---

# 34. Blog Index

Bun’s blog index begins with a featured/latest article, then lets users filter Everything / Articles / Releases, followed by a chronological archive grouped by year.

Template:

```text
# Blog

┌─────────────────────────────────────────────┐
│ LATEST                                      │
│                                             │
│ Product 2.0                                 │
│ Large description...                       │
│                                             │
│ Aug 20, 2026 · Release              →       │
└─────────────────────────────────────────────┘

Everything   Articles   Releases

176 posts

## 2026

Jul 8    Article title                 Author
May 13   Version 1.9                   Release
Apr 20   Version 1.8                   Release
```

Excellent pattern for products with frequent engineering releases.

Do not turn every archive entry into a card.

---

# 35. Blog Article

Current Bun long-form articles use extremely long technical prose, standard headings, code examples, tables and media while retaining the same global site shell.

Template:

```text
Blog

# Major Engineering Story

[Author]
July 8, 2026

Lead paragraph...

Body...

## Heading

Body...

code / figure

### Subheading

...
```

Article content width:

```text
680–760px
```

Images/code may break outward:

```text
840–1100px
```

Use large margins between conceptual sections.

---

# 36. Release Article

A release post should be more structured than a normal article.

Recommended:

```text
Product v2.0

date · Release

Lead summary

[ Upgrade command ]

## Headline feature
demo

## Performance
benchmarks

## New APIs
code

## Compatibility
table

## Fixes
categorized list

## Contributors
...
```

Treat release notes as a launch page inside the editorial system.

This is one reason Bun’s blog architecture scales well: releases and editorial engineering articles live together but remain filterable.

---

# 37. Cards

Do not use one generic card for everything.

Create semantic card variants:

| Variant        | Purpose                 |
| -------------- | ----------------------- |
| `FeatureCard`  | Product capability      |
| `GuideCard`    | Navigation              |
| `ProofCard`    | Benchmark/result        |
| `StoryCard`    | Customer example        |
| `ReleaseCard`  | Latest release          |
| `ModuleCard`   | API namespace           |
| `CalloutCard`  | Documentation notice    |
| `TerminalCard` | CLI output              |
| `CodeCard`     | Code                    |
| `MetricCard`   | One quantitative result |

Shared anatomy:

```text
border
12px radius
20–24px padding
no/default shadow
```

But content rules differ.

---

# 38. Badges

Use badges for metadata, not decoration.

Examples:

```text
NEW
STABLE
EXPERIMENTAL
v2.0
RELEASE
FUNCTION
```

Recommended:

```text
font: mono
font-size: 11–12
font-weight: 600
padding: 4px 8px
pill radius
pink-soft background
dark-pink foreground
```

---

# 39. Tables

Technical tables are core UI.

Rules:

```text
no vertical rules by default
hairline horizontal dividers
left aligned text
right aligned numeric values
tabular numeric font
sticky headers for long tables
subtle selected row
```

Header:

```text
12–13px
600
muted
```

Rows:

```text
14–15px
```

---

# 40. Copy Style

The copy should sound like an engineer demonstrating something.

## Headline

Short:

```text
One binary for the whole workflow.
```

Not:

```text
Revolutionize your development workflow with our next-generation platform.
```

## Supporting copy

Concrete:

```text
Compile, test and deploy using the same tool.
```

Not:

```text
Unlock unparalleled developer productivity.
```

## Technical proof

Prefer:

```text
210ms
12 MB
1,400 tests
```

over:

```text
Blazing fast
Lightweight
Highly compatible
```

---

# 41. Section Naming

Small editorial eyebrows can introduce sections:

```text
IN PRODUCTION

PACKAGE MANAGEMENT

BATTERIES INCLUDED

LATEST RELEASE
```

Then use a conversational H2:

```text
Everything built in. Nothing weighing it down.
```

Pattern:

```text
technical label
+
human headline
+
concrete body
+
working example
```

Very Bun-like.

---

# 42. Illustration Strategy

For a truly project-agnostic interpretation:

Do **not** reuse Bun’s mascot.

Instead choose one recurring identity mechanism:

```text
character
object
abstract mark
physical metaphor
tiny diagram language
```

The key lesson from Bun is not “use a cute mascot.”

It is:

**let one recognizable element carry most of the personality while the interface remains restrained.**

If your product has no mascot, use:

* a geometric object
* a physical tool metaphor
* tiny line illustrations
* one distinctive glyph
* product-specific diagrams

Avoid generic 3D blobs.

---

# 43. Iconography

Icons are optional.

Technical labels and typography should do most of the work.

Recommended icon style if needed:

```text
16px
1.5–2px stroke
rounded but not bubbly
single color
```

Use icons mainly for:

* search
* external link
* copy
* GitHub
* disclosure
* menu
* theme controls

Don't put an icon next to every heading.

---

# 44. Motion

Recommended tokens:

```css
--duration-fast: 120ms;
--duration-base: 200ms;
--duration-slow: 320ms;

--ease-standard: cubic-bezier(.4,0,.2,1);
--ease-emphasized: cubic-bezier(.2,0,0,1);
```

Allowed motion:

```text
button translateY(-1px)
card translateY(-1px)
tab fades
terminal playback
benchmark bar animation
code copy feedback
accordion expansion
subtle illustration movement
```

Avoid:

* floating everything
* scroll hijacking
* huge parallax
* mouse-following gradient blobs
* constant glow animation

The interface should still feel extremely fast when motion is removed.

---

# 45. Responsive System

## ≥1280px

Full marketing grid.

Docs:

```text
sidebar + content + TOC
```

## 1024–1279px

Docs:

```text
sidebar + content
TOC hidden/collapsible
```

Marketing:

```text
4-column → 2-column where appropriate
```

## 640–1023px

Sidebar moves to drawer.

Hero:

```text
two-column → stacked
```

Benchmark modules become full width.

## <640px

```text
16px page gutters
44px minimum hit targets
H1 ~44px
Hero ~48–56px
cards full width
tabs horizontally scroll
tables horizontally scroll
terminal horizontally scroll
```

Do not shrink code until unreadable.

Allow horizontal code scrolling.

---

# 46. Marketing Homepage Template

The current Bun homepage flows through installation, benchmarks, the four-tool proposition, workflow demonstration, latest release, production proof, more benchmarks, built-in APIs, frontend/full-stack capability, comparison matrix and a final install CTA.

Use this generalized sequence:

```text
01 Global announcement
02 Header
03 Hero
04 Immediate install / try-it control
05 High-impact benchmark
06 Customer / ecosystem logos
07 Core product pillars
08 Interactive workflow
09 Latest release
10 Production proof
11 Deep benchmark/evidence section
12 Built-in capabilities explorer
13 Secondary product workflow
14 Feature comparison table
15 Final CTA
16 Footer
```

This is an unusually strong homepage architecture for developer products because it alternates:

```text
claim
→ proof
→ explanation
→ proof
→ workflow
→ social proof
→ detailed proof
```

instead of:

```text
hero
→ features
→ logos
→ pricing
```

---

# 47. Documentation Page Template

```text
Global nav
────────────────────────────────────────────

Sidebar     Breadcrumb       TOC
            H1
            Lead

            Code

            Body

            ─────────────────

            H2

            Body

            Code

            ─────────────────

            H2

            ...

            Previous / Next

            Edit / Markdown
```

---

# 48. Guides Index Template

```text
Global nav
────────────────────────────────────────────

Sidebar

            Guides
            Short explanation

            Featured
            ┌───────┐ ┌───────┐ ┌───────┐

            Deployment
            link
            link
            link

            Frameworks
            link
            link
            link

            Networking
            ...
```

---

# 49. Guide Detail Template

```text
Global nav
────────────────────────────────────────────

Sidebar

            Breadcrumb

            H1
            one paragraph

            filename
            CODE

            ─────────

            instruction

            terminal
            COMMAND

            ─────────

            instruction

            terminal
            COMMAND

            Previous / Next
```

---

# 50. API Reference Template

```text
Global nav
────────────────────────────────────────────

Reference tree   Breadcrumb

                 symbol kind
                 SymbolName<T>

                 signature

                 description

                 ───────────

                 property
                 description
                 example

                 ───────────

                 property
                 ...
```

Add:

```text
Browse symbols
/
⌘K
```

as first-class actions.

---

# 51. Blog Index Template

```text
Global nav

Blog

Featured latest article

Everything / Articles / Releases

post count

2026
date     title                         type author
date     title                         type author
date     title                         type author

2025
...
```

---

# 52. Blog Article Template

```text
Global nav

Blog

H1

author · date · metadata

lead

body

large technical media

H2

body

code

H2

table

body

related article / footer
```

---

# 53. Component Inventory

For implementation, build these primitives:

```text
AppShell
GlobalHeader
AnnouncementBar
Container
Prose
Section
SectionHeader
Eyebrow
Button
TextLink
Badge
Tabs
Card
FeatureCard
Metric
MetricGrid
Terminal
CodeBlock
CodeTabs
CopyButton
Benchmark
BenchmarkRow
ComparisonTable
WorkflowStepper
FeatureExplorer
Testimonial
DocsShell
DocsSidebar
DocsNavGroup
Breadcrumbs
TableOfContents
Callout
PrevNext
SearchTrigger
SearchDialog
GuideIndex
GuideLink
SymbolBadge
SymbolSearch
Signature
MemberDefinition
BlogHero
BlogFilter
BlogArchive
ArticleMeta
Article
Footer
```

You should resist adding much more until a real page requires it.

---

# 54. CSS Foundation

```css
:root {
  --canvas: #fbf0df;
  --canvas-soft: #fff7e8;
  --canvas-strong: #f5e8d0;

  --surface: #ffffff;
  --surface-soft: #fdfaf2;

  --foreground: #15151b;
  --foreground-strong: #000;
  --muted: #6b6b6b;
  --faint: #9a9a9a;

  --brand: #f472b6;
  --brand-hover: #db2777;
  --brand-soft: #fce7f3;
  --brand-faint: #fdf2f8;

  --accent: #fbbf24;
  --accent-hover: #f59e0b;
  --accent-soft: #fef3c7;

  --border: rgb(21 21 27 / 10%);
  --border-strong: rgb(21 21 27 / 20%);
  --border-subtle: rgb(21 21 27 / 5%);
  --divider: rgb(21 21 27 / 8%);

  --code-bg: #15151b;
  --code-fg: #fbf0df;
  --code-muted: #a1a1aa;

  --radius-sm: 4px;
  --radius-md: 6px;
  --radius-lg: 8px;
  --radius-card: 12px;
  --radius-xl: 16px;

  --content: 1200px;
  --prose: 700px;
  --docs: 1480px;

  --header-height: 64px;

  --duration-fast: 120ms;
  --duration-base: 200ms;
  --duration-slow: 320ms;

  --ease-standard: cubic-bezier(.4, 0, .2, 1);
  --ease-emphasized: cubic-bezier(.2, 0, 0, 1);
}
```

---

# 55. Base Surface

```css
body {
  margin: 0;
  background: var(--canvas);
  color: var(--foreground);
  font-family: var(--font-body);
  font-size: 17px;
  line-height: 1.6;
}
```

The cream background is important.

Don't replace it with:

```text
#fafafa
#ffffff
zinc-950
```

and expect the same personality.

---

# 56. Generic Card

```css
.card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  padding: 24px;
}
```

No default box shadow.

---

# 57. Generic Button

```css
.button-primary {
  display: inline-flex;
  min-height: 44px;
  align-items: center;
  justify-content: center;

  padding: 0 22px;

  border: 0;
  border-radius: 8px;

  background: var(--foreground);
  color: var(--canvas);

  font-weight: 700;

  transition:
    transform var(--duration-fast) var(--ease-standard),
    background var(--duration-fast) var(--ease-standard);
}

.button-primary:hover {
  background: #000;
  transform: translateY(-1px);
}
```

---

# 58. Generic Code Block

```css
.code-block {
  overflow-x: auto;

  padding: 20px 24px;

  border-radius: 12px;

  background: var(--code-bg);
  color: var(--code-fg);

  font-family: var(--font-mono);
  font-size: 14px;
  line-height: 1.6;
}
```

---

# 59. What Makes This System Feel Like Bun

It is **not** primarily:

```text
cream
pink
rounded cards
```

Those are superficial.

The real ingredients are:

```text
1. Warm editorial canvas
2. Aggressive display typography
3. Technical artifacts as visual content
4. Heavy use of real commands and real numbers
5. Dense documentation hierarchy
6. Clear separation of Docs / Guides / Reference
7. Border-driven rather than shadow-driven depth
8. Monospace used structurally
9. Brand personality concentrated in a few places
10. Long pages built from alternating explanation and evidence
```

---

# 60. What Not To Copy

Do not carry these Bun-specific elements into an unrelated project:

```text
Bun logo
Bun mascot
exact Bun marketing copy
Bun-specific pink illustrations
Bun-specific benchmark datasets
Bun command examples
Bun product taxonomy
```

Instead preserve:

```text
warm technical atmosphere
page architecture
evidence-driven sections
terminal treatment
documentation density
typographic hierarchy
paper-like depth
single-accent discipline
```

That gives you a system **derived from Bun instead of a Bun impersonation**.

---

# 61. Rules for a Successful Implementation

### DO

* Let technical content become the artwork.
* Use real code instead of fake decorative snippets.
* Use actual benchmarks when making performance claims.
* Keep pages warm and light.
* Use dark primarily for code/terminal.
* Use strong display typography sparingly.
* Keep body copy straightforward.
* Build very good documentation navigation.
* Use tables whenever the content is inherently tabular.
* Separate conceptual docs from task-oriented guides.
* Give API reference its own denser information model.
* Allow long editorial pages.
* Keep border radii moderate.
* Keep shadows rare.
* Make search excellent.

### DON'T

* Add gradients everywhere.
* Use a different pastel for every section.
* Put every feature inside a card.
* Use huge icons.
* Make all elements pill-shaped.
* Turn docs into a marketing page.
* Turn guide indexes into a three-card landing page.
* Use massive code fonts.
* hide technical details behind animations.
* use pink as the primary color on every control.
* make dark mode the defining aesthetic.
* add gratuitous glass effects.
* use generic “developer experience reimagined” copy.

---

# 62. Final Visual Formula

If you had to recreate the whole language with a single formula:

```text
Cream paper
+
near-black oversized grotesk
+
precise neutral body text
+
dark monospace technical artifacts
+
hairline borders
+
white paper cards
+
one playful pink accent
+
real engineering evidence
+
dense but beautifully organized documentation
```

That is the system.
