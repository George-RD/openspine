# OpenSpine design system

## Direction

**Safety-case control instrument.** OpenSpine should feel like a clear technical object used to inspect and govern authority: a safety case, a control panel, and an engineering verification record in one visual language.

The system refuses the category default of dark neon “AI security.” It uses a bright working surface for reading, a deep-ink instrument for runtime activity, and safety colors with fixed semantic roles.

## Composition

- The landing page is asymmetric and mechanism-first.
- The first viewport pairs a plain-language offer with a large authority trace that visibly denies `email.send`.
- Dense technical information lives in ruled ledgers, traces, and panels rather than generic cards.
- Sections alternate between the pale working surface, a deep control-boundary field, and a safety-orange activation field.
- Square edges and clipped corners imply fabricated equipment, not a SaaS template.
- Hard offset shadows establish physical depth. Avoid soft floating card stacks.

## Color roles

| Token | Value | Role |
| --- | --- | --- |
| Paper | `#eef3f0` | Main daylight working surface |
| Raised paper | `#f8fbf8` | Letters, docs, and proof surfaces |
| Deep paper | `#dce7e1` | Secondary fields and ruled bands |
| Ink | `#0b1c24` | Primary text and runtime instrument |
| Ink 2 | `#19323a` | Instrument headers and secondary dark fields |
| Ink 3 | `#385058` | Body copy on pale surfaces |
| Safety orange | `#ff6438` | Authority, action, denial, and activation |
| Deep orange | `#b52f10` | Accessible emphasis on pale surfaces |
| Pale orange | `#ffd8c9` | Alpha and denied-state labels |
| Verification mint | `#8cdec6` | Verified source and allowed effect |
| Deep mint | `#0e6d61` | Accessible verified status on pale surfaces |
| Approval yellow | `#f3d56b` | Approval-required and untrusted-content attention |
| Deep yellow | `#684f00` | Accessible text on yellow |

Color is semantic. Do not decorate arbitrary elements with safety colors.

## Typography

- **Display:** Barlow Condensed, weights 500–700. Use for the brand, major headings, and ledger row titles. Its narrow industrial proportion gives the page authority without pretending to be code.
- **Body:** Atkinson Hyperlegible, weights 400 and 700. Use for prose, navigation, controls, and documentation.
- **Monospace:** system monospace only for action identifiers, test names, hashes, trace IDs, and terminal commands.
- Display headings are tight but never below `-0.03em` tracking.
- Body measure stays near 65–72 characters where possible.

## Core components

### OpenSpine mark

A rectangular frame, three horizontal rails, and three state nodes. The middle node is verified mint; the outer nodes are safety orange. It is an abstract runtime instrument, not a literal anatomical spine.

### Instrument panel

Deep ink, square construction, one clipped top corner, internal hairlines, and a hard offset shadow. It hosts live mechanism demonstrations. Motion may illuminate or traverse its existing structure; it may not become decorative spectacle.

### Authority rail

A thin line connecting named runtime stages. The animation travels in one direction, wakes each stage, composes policy layers, issues a task grant, then reveals gate decisions and the audit record.

### Policy layers and task grant

Inputs appear as restrained horizontal records. The resolved grant is safety orange because it is the only live authority object. Show scope, budget, and expiry rather than invented metrics.

### Gate ledger

A compact ruled list with fixed state colors:

- Allow = verification mint
- Ask = approval yellow
- Deny = safety orange

Action identifiers are monospace. The reason remains readable text.

### Claim ledger

A full-width technical table with three fields: risk, runtime result, and named test. On narrow screens, each row becomes a stacked record with the test preserved in full.

### Buttons

Rectangular with no pill radius. Primary controls use solid fields and a hard offset shadow. Hover moves the button toward its shadow rather than adding glow. Secondary controls invert or fill; they do not become translucent glass.

## Motion

The authored moment is the first authority trace:

1. A verified request appears already present.
2. A pulse moves through the seven runtime stages.
3. Policy inputs converge into a scoped task grant.
4. The gate ledger reveals allow, ask, and deny in sequence.
5. The audit node confirms the record.

Motion uses exponential ease-out, clip, blur, and position. No bounce or elastic easing. A replay control is visible. Fine-pointer devices may add a very small instrument tilt. All animation is removed under `prefers-reduced-motion`.

Other sections use one restrained reveal when entering the viewport. Do not apply identical animation to every child.

## Responsive behavior

- Desktop keeps the asymmetric hero and dense authority trace.
- Tablet stacks the offer above the trace, then keeps the mechanism ledgers intact.
- The Lyra three-pane workflow becomes a vertical sequence with directional connectors.
- Mobile turns the seven-stage rail into a compact two-row stage map and stacks authority composition before the grant.
- Tables become records rather than forcing unreadable horizontal compression.
- Primary actions become full-width only on the smallest screens.

## Documentation

Starlight documentation inherits the same type, paper, ink, orange, and ruled-table language. Documentation remains calmer than the landing page. It does not reproduce the large instrument effects or promotional composition.

## Quality floor

- Body text contrast is at least 4.5:1; large text at least 3:1.
- Keyboard focus uses a visible safety-orange outline.
- Controls have hover, focus, disabled, and failure states where applicable.
- The page works without JavaScript; JavaScript enhances reveal, replay, copy, and header behavior.
- Illustrative interface content is labelled as illustrative.
- No commercial, adoption, benchmark, or capability claim is invented for visual effect.
- No gradient text, decorative glass, glowing borders, nested cards, or generic icon-card grids.
