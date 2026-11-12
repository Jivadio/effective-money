# Solde — Design System (Ledger direction)

> The reference for designing and building the Solde interface. Source of truth: the tokens below. Always use the CSS variables, never hard-coded values.

This is the design system for Solde, a personal finance app covering accounts, transactions, budgets and savings. Solde reads your accounts; it never moves money, so there are no transfers or payments in the UI. The **Ledger** direction is precise, calm and legible. It has one goal: **users know where they stand within three seconds**, and feel safe with their money.

Two themes: **Light** (default) and **Dark**. Same roles, same token names.

The UI ships in five languages (**EFIGS**: English, French, Italian, German, Spanish). English is the source locale. Everything here applies to every language.

## Principles

1. **The number comes first.** One hero amount per screen (`amount-xl`). Everything else is subordinate to it, and nothing decorative sits next to a balance.
2. **Ink, not colour.** The interface is black and off-white, and the accent is the ink itself (`accent` = `ink`). Colour is reserved for **meaning**: `positive`, `negative`, `warning`.
3. **One lime.** `highlight` (#c8f04a) is Solde's identity: a healthy gauge, a goal reached, the active tab, the main chart series. In the light theme it never carries text (1.3:1 on white). It is a fill, with `on-highlight` on top.
4. **Never colour alone.** `+€120.00` / `−€42.90` (real minus sign, U+2212), "Almost reached" spelled out, an arrow on deltas. Readable by colour-blind users and in greyscale.
5. **Calm by default.** An ordinary expense is in `ink`, not red. `negative` means overdraft, overspending, a penalty or an error.
6. **Visible trust.** Sync freshness is always shown, every hero amount has a discreet mode (eye), every action that talks to a bank (sync, reconnect) shows `loading`, and anything irreversible asks for confirmation.

## Voice

Short sentences, concrete numbers. Say what is happening and what to do. Write the English source string first; translations keep the same tone.

| Yes | No |
|---|---|
| Rent of €1,100.00 due tomorrow. Your balance covers it. | Warning! Upcoming transaction on your account. |
| Over by €22 | Budget exceeded (122%) |
| Reconnect Banque Populaire to update your accounts. | Error 503: aggregation failed. |
| Reconnect bank | Submit |

### Localization (EFIGS)

- **Address**: direct "you" in English. Use the formal form where the language has one: *vous* (FR), *Sie* (DE), *Lei* (IT), *usted* (ES).
- **Amounts and dates are formatted by the user's locale**, never by hand: `Intl.NumberFormat` and `Intl.DateTimeFormat` / `Intl.RelativeTimeFormat`. The same amount renders as `€1,240.00` (en), `1 240,00 €` (fr), `1.240,00 €` (de), `1240,00 €` (it and es: no grouping below 10 000). The currency comes from the account, not from the locale.
- **Dates**: relative up to "Yesterday", then a short date (`Oct 28`, `28 oct.`, `28. Okt.`).
- **Strings**: ICU message format, one key per sentence, plurals through ICU. Never concatenate translated fragments, and never put text in images.
- **Text expansion**: German and French run about 30% longer than English. Buttons, tabs and badges must wrap or grow, never truncate a label. Test every screen in German.
- Labels are verb + object in every language ("Add an account", "Ajouter un compte", "Konto hinzufügen").

## Colours

- **Backgrounds**: `bg` → `surface` (cards) → `surface-sunken` (tracks, chips, hover). Use `surface-inverse` + `on-inverse` for an inverted hero card, at most one per screen.
- **Text**: `ink` (≥ 15:1), `ink-muted` (≥ 4.5:1 on `bg`, `surface` and `surface-sunken` in both themes), `ink-disabled` for disabled content only.
- **Action**: `accent` / `accent-hover` / `on-accent`. In dark mode the accent inverts (off-white button, black text).
- **Meaning**: `positive`, `negative`, `warning`, each with a `-soft` background for badges and alerts (text ≥ 4.5:1 on its background).
- **Borders**: `border` is decorative. `border-strong` is for fields and secondary buttons (≥ 3:1).
- **Focus**: `focus`, the only blue in the system, 2 px offset by 2 px, on every interactive element.
- **Charts**: `chart-1` (lime) to `chart-5` ("Other", neutral). Segments are separated by 2 px, and a legend with values is mandatory.

## Typography

Inter, a single family, always with tabular figures (`font-variant-numeric: tabular-nums`). Inter covers every EFIGS character.

- **Amounts**: `amount-xl` (40), `amount-lg` (28), `amount-md` (16), `amount-sm` (13). In `xl`/`lg`, the cents are in `ink-muted`.
- **Text**: `title` (24, one per screen), `heading` (16, card titles), `body` (15), `label` (13, buttons and fields), `caption` (12, metadata), `overline` (11, capitals, only above a hero amount).

## Spacing, shapes, sizes

A 4 px grid: `space-1` (4) … `space-12` (48). Cards: padding `space-4` (`space-5` for the hero card), gap `space-4`. Radii: `radius-sm` badges, `radius-md` buttons and fields, `radius-lg` cards, `radius-xl` sheets, `radius-pill` gauges and the segmented control. Controls: `control-md` is 44 px minimum (touch target), `control-lg` 56 px for amount entry.

Elevation: the border does the work. `shadow-card` is almost invisible. `shadow-pop` is reserved for what floats (menus, toasts, sheets). In dark mode, cards have no shadow.

## Iconography

A provisional in-house set (`Icon`): 1.75 px stroke, 24 grid, round caps, in `currentColor`. In a transaction row, the category icon sits on a 40 px `surface-sunken` tile (`icon-tile`). An icon never carries meaning alone: add a label or an `aria-label`.

No logo yet: the name is set in Inter 600, `ink`.

## Components

`Button`, `Amount`, `Badge`, `Icon`, `Card`, `SegmentedControl`, `AmountField`, `TransactionRow`, `BalanceCard`, `BudgetGauge`, `CategoryBar`, `Notice`, `TabBar`, and the **Home** reference screen. They are React components whose styles read only the token variables. They will live in `apps/web` once the UI work starts.

## Accessibility

Contrast is checked in both themes (text 4.5:1, controls and focus 3:1). Targets are 44 px. ARIA roles on gauges (`progressbar`), segmented controls (`radiogroup`), alerts (`alert`/`status`) and discreet mode (`aria-pressed`). Animations are reduced under `prefers-reduced-motion`. Every accessible name (`aria-label`, alt text) is translated like any other string, and `<html lang>` follows the active locale.


---

# Tokens

## Colours

Themes: `light` (default) and `dark`, applied through `[data-theme="…"]` on `<html>`.

| Token | Light | Dark | Usage |
|---|---|---|---|
| `--bg` | `#f5f5f2` | `#0e0f11` | Page background. `surface` cards on top create the hierarchy, not shadows. |
| `--surface` | `#ffffff` | `#17191c` | Cards, sheets, transaction lists, fields. |
| `--surface-sunken` | `#ebebe6` | `#0a0b0c` | Recessed areas: gauge tracks, segmented control, chips, row hover. |
| `--surface-inverse` | `#121417` | `#f2f2ee` | Inverted hero card (balance, bank card), toast. Text on top: `on-inverse`. |
| `--on-inverse` | `#f5f5f2` | `#121417` | Text on `surface-inverse`. |
| `--border` | `#e3e3dc` | `#2a2d32` | Row separators and card outlines. Decorative, never the only carrier of information. |
| `--border-strong` | `#8a8f96` | `#6b717a` | Outline of fields and secondary buttons (≥ 3:1 on `surface`). |
| `--ink` | `#121417` | `#f2f2ee` | Main text and ordinary amounts (expenses included). |
| `--ink-muted` | `#5a5f66` | `#9aa0a8` | Labels, dates, metadata, placeholders (≥ 4.5:1 on `bg` and `surface`). |
| `--ink-disabled` | `#a3a7ad` | `#55595f` | Disabled text and icons only (exempt from contrast). |
| `--accent` | `#121417` | `#f2f2ee` | Primary button, active selection, active tab. The Ledger accent is the ink: black in light, off-white in dark. |
| `--accent-hover` | `#2b2f35` | `#d9d9d3` | Hover on `accent`. |
| `--on-accent` | `#ffffff` | `#121417` | Text and icons on `accent`. |
| `--highlight` | `#c8f04a` | `#c8f04a` | The Ledger lime: identity, goal reached, healthy gauge, main chart series. In light: fill only, never text. |
| `--on-highlight` | `#121417` | `#121417` | Text on `highlight`. |
| `--highlight-soft` | `#eef9c8` | `#2a3311` | "Goal" badge background, soft emphasis. |
| `--positive` | `#0a7a45` | `#4cc38a` | Income, gains, balance increase. Always with the + sign. |
| `--positive-soft` | `#e3f3ea` | `#13281d` | Positive badge background. |
| `--negative` | `#c0391f` | `#ff7a63` | Overspending, overdraft, loss, error. Not an ordinary expense. Always with the − sign. |
| `--negative-soft` | `#fbe7e3` | `#331512` | Negative badge and alert background. |
| `--warning` | `#8f5e08` | `#e8b04a` | Budget almost reached (≥ 80%), upcoming direct debit. |
| `--warning-soft` | `#f8eed9` | `#2e230d` | Warning badge and alert background. |
| `--focus` | `#1f5fd6` | `#7aa7ff` | Keyboard focus ring, 2px, offset by 2px. The only blue in the system, so it is seen everywhere. |
| `--chart-1` | `{highlight}` | `{highlight}` | Charts: main series / 1st category. |
| `--chart-2` | `#2c4a3e` | `#7fb59c` | Charts: 2nd category. |
| `--chart-3` | `#7f93a8` | `#8fa6be` | Charts: 3rd category. |
| `--chart-4` | `#c99a3c` | `#d9ad55` | Charts: 4th category. |
| `--chart-5` | `#c9c9c1` | `#4a4e55` | Charts: "Other", always last. |
| `--scrim` | `rgba(18,20,23,0.4)` | `rgba(0,0,0,0.6)` | Veil behind sheets and modals. |

## Typography

Family: `--font-sans: Inter, system-ui, -apple-system, "Segoe UI", Roboto, sans-serif`. Always `font-variant-numeric: tabular-nums`.

| Style | Size / line height | Weight | Tracking | Example (en) | Usage |
|---|---|---|---|---|---|
| `.amount-xl` | 40px / 44px | 600 | -0.025em | €12,480.35 | Hero balance, one per screen. Tabular figures. |
| `.amount-lg` | 28px / 32px | 600 | -0.02em | €2,000.00 | Card header amount: budget, savings goal, entry. |
| `.amount-md` | 16px / 22px | 600 | 0 | −€42.90 | Transaction or account row amount, right-aligned. |
| `.amount-sm` | 13px / 18px | 500 | 0 | +€312.40 | Deltas, secondary amounts within a label. |
| `.title` | 24px / 30px | 600 | -0.015em | November budget | Screen title, one per screen. |
| `.heading` | 16px / 22px | 600 | -0.005em | Recent transactions | Card or section title. |
| `.body` | 15px / 22px | 400 | 0 | Your rent will be debited in 3 days. | Body text, main label of a row. |
| `.label` | 13px / 18px | 500 | 0 | Add an account | Buttons, tabs, field labels. |
| `.caption` | 12px / 16px | 400 | 0 | Synced 2 min ago | Dates, metadata, hints, notes. |
| `.overline` | 11px / 14px | 600 | 0.06em | TOTAL BALANCE | Capitalised over-title above a hero amount. Sparingly. |

## Spacing

| Token | Value | Usage |
|---|---|---|
| `--space-1` | `4px` | Icon ↔ text, amount ↔ caption. |
| `--space-2` | `8px` | Chip and badge padding, gap between buttons. |
| `--space-3` | `12px` | Vertical padding of a list row. |
| `--space-4` | `16px` | Card padding, mobile gutter. |
| `--space-5` | `20px` | Hero card padding. |
| `--space-6` | `24px` | Between two cards. |
| `--space-8` | `32px` | Between two screen sections. |
| `--space-12` | `48px` | Top of screen, empty states. |

## Radii

| Token | Value | Usage |
|---|---|---|
| `--radius-sm` | `6px` | Badges, chips, small category icons. |
| `--radius-md` | `12px` | Buttons, fields, 40px icon tiles. |
| `--radius-lg` | `20px` | Cards. |
| `--radius-xl` | `28px` | Sheets (bottom sheets), modals. |
| `--radius-pill` | `999px` | Gauges, segmented control, toggles. |

## Control sizes

| Token | Value | Usage |
|---|---|---|
| `--control-sm` | `32px` | Compact button height, segmented control. |
| `--control-md` | `44px` | Standard button and field height: minimum touch target. |
| `--control-lg` | `56px` | Amount entry field, full-width button at the bottom of the screen. |
| `--icon-tile` | `40px` | Category icon tile in a row. |

## Shadows

| Token | Value | Usage |
|---|---|---|
| `--shadow-card` | light `0 1px 2px rgba(18,20,23,0.05)` · dark `none` | Cards at rest. Almost invisible: the border does the work. |
| `--shadow-pop` | light `0 8px 24px rgba(18,20,23,0.12), 0 2px 6px rgba(18,20,23,0.06)` · dark `0 8px 24px rgba(0,0,0,0.5)` | Menus, toasts, sheets: whatever floats above the content. |

## Tokens in CSS (ready to copy)

```css
:root, [data-theme="light"] {
  --bg: #f5f5f2;
  --surface: #ffffff;
  --surface-sunken: #ebebe6;
  --surface-inverse: #121417;
  --on-inverse: #f5f5f2;
  --border: #e3e3dc;
  --border-strong: #8a8f96;
  --ink: #121417;
  --ink-muted: #5a5f66;
  --ink-disabled: #a3a7ad;
  --accent: #121417;
  --accent-hover: #2b2f35;
  --on-accent: #ffffff;
  --highlight: #c8f04a;
  --on-highlight: #121417;
  --highlight-soft: #eef9c8;
  --positive: #0a7a45;
  --positive-soft: #e3f3ea;
  --negative: #c0391f;
  --negative-soft: #fbe7e3;
  --warning: #8f5e08;
  --warning-soft: #f8eed9;
  --focus: #1f5fd6;
  --chart-1: var(--highlight);
  --chart-2: #2c4a3e;
  --chart-3: #7f93a8;
  --chart-4: #c99a3c;
  --chart-5: #c9c9c1;
  --scrim: rgba(18,20,23,0.4);
  --shadow-card: 0 1px 2px rgba(18,20,23,0.05);
  --shadow-pop: 0 8px 24px rgba(18,20,23,0.12), 0 2px 6px rgba(18,20,23,0.06);
}
[data-theme="dark"] {
  --bg: #0e0f11;
  --surface: #17191c;
  --surface-sunken: #0a0b0c;
  --surface-inverse: #f2f2ee;
  --on-inverse: #121417;
  --border: #2a2d32;
  --border-strong: #6b717a;
  --ink: #f2f2ee;
  --ink-muted: #9aa0a8;
  --ink-disabled: #55595f;
  --accent: #f2f2ee;
  --accent-hover: #d9d9d3;
  --on-accent: #121417;
  --highlight: #c8f04a;
  --on-highlight: #121417;
  --highlight-soft: #2a3311;
  --positive: #4cc38a;
  --positive-soft: #13281d;
  --negative: #ff7a63;
  --negative-soft: #331512;
  --warning: #e8b04a;
  --warning-soft: #2e230d;
  --focus: #7aa7ff;
  --chart-1: var(--highlight);
  --chart-2: #7fb59c;
  --chart-3: #8fa6be;
  --chart-4: #d9ad55;
  --chart-5: #4a4e55;
  --scrim: rgba(0,0,0,0.6);
  --shadow-card: none;
  --shadow-pop: 0 8px 24px rgba(0,0,0,0.5);
}
:root {
  --font-sans: Inter, system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-5: 20px;
  --space-6: 24px;
  --space-8: 32px;
  --space-12: 48px;
  --radius-sm: 6px;
  --radius-md: 12px;
  --radius-lg: 20px;
  --radius-xl: 28px;
  --radius-pill: 999px;
  --control-sm: 32px;
  --control-md: 44px;
  --control-lg: 56px;
  --icon-tile: 40px;
}
.amount-xl { font: 600 40px/44px var(--font-sans); letter-spacing: -0.025em; font-variant-numeric: tabular-nums; }
.amount-lg { font: 600 28px/32px var(--font-sans); letter-spacing: -0.02em; font-variant-numeric: tabular-nums; }
.amount-md { font: 600 16px/22px var(--font-sans); font-variant-numeric: tabular-nums; }
.amount-sm { font: 500 13px/18px var(--font-sans); font-variant-numeric: tabular-nums; }
.title { font: 600 24px/30px var(--font-sans); letter-spacing: -0.015em; font-variant-numeric: tabular-nums; }
.heading { font: 600 16px/22px var(--font-sans); letter-spacing: -0.005em; font-variant-numeric: tabular-nums; }
.body { font: 400 15px/22px var(--font-sans); font-variant-numeric: tabular-nums; }
.label { font: 500 13px/18px var(--font-sans); font-variant-numeric: tabular-nums; }
.caption { font: 400 12px/16px var(--font-sans); font-variant-numeric: tabular-nums; }
.overline { font: 600 11px/14px var(--font-sans); letter-spacing: 0.06em; text-transform: uppercase; font-variant-numeric: tabular-nums; }
```


---

# Components

React 18. CSS class prefix: `sd-`. Styles only read the variables above. Every visible string and accessible name comes from the translation catalogue; components never embed copy.

## Button

Triggers an action. One `primary` button per screen or per card.

- `variant`: `primary` (`accent` background), `secondary` (`border-strong` outline), `ghost` (tertiary actions, toolbars), `highlight` (celebrate: confirming a goal; rare), `danger` (irreversible, always followed by a confirmation).
- `size`: `md` 44 px by default (touch target), `sm` 32 px in dense lists, `lg` 56 px for the full-width action at the bottom of the screen (`block`).
- `icon` left of the label. An icon-only button requires `aria-label`.
- `loading` disables the button and sets `aria-busy`. Use it while a bank request runs (sync, reconnect, adding an account) to prevent double clicks.
- Labels are verb + object: "Add an account", never "OK". The label wraps rather than truncating (German).

```ts
interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary'|'secondary'|'ghost'|'highlight'|'danger';
  size?: 'sm'|'md'|'lg';
  icon?: IconName;
  iconRight?: IconName;
  loading?: boolean;
  block?: boolean
}
```

## Amount

Displays an amount formatted for the active locale, with tabular figures and a real minus sign.

- `size`: `xl` hero balance (one per screen), `lg` card header, `md` rows, `sm` deltas.
- In `xl`/`lg`, the minor units are in `ink-muted` so the eye reads the major units first. This applies wherever the locale puts the decimals.
- `signed` shows `+` and colours values > 0 in `positive` (income, gains). An ordinary expense stays in `ink`.
- `alertNegative` colours in `negative`: only for an overdraft, a penalty, overspending.
- `hidden` → `••••` with the currency symbol in its locale position (discreet mode), with an `aria-label`.
- The consumer passes a number, never a pre-formatted string. `formatAmount(value, { locale, signed, decimals, currency })` is exported for text.

```ts
interface AmountProps {
  value: number;
  size?: 'xl'|'lg'|'md'|'sm';
  signed?: boolean;
  tone?: 'auto'|'neutral'|'positive'|'negative';
  alertNegative?: boolean;
  hidden?: boolean;
  currency?: string;
  decimals?: number;
  className?: string
}
```

## Badge

A short label (1 to 3 words): category, status, delta.

- `tone`: `neutral` (categories), `positive`, `negative`, `warning`, `highlight` (goal reached; rare).
- Always a word or a signed number: colour never carries meaning alone. Use `dot` or `icon` as reinforcement.

```ts
interface BadgeProps {
  tone?: 'neutral'|'positive'|'negative'|'warning'|'highlight';
  icon?: IconName;
  dot?: boolean;
  children?: React.ReactNode
}
```

## Icon

The Solde icon set: 1.75 px stroke, 24 grid, round caps, `currentColor`.

- Sizes: 20 px in tiles and the tab bar, 18 px in buttons, 14–16 px in badges and metadata.
- Decorative by default (`aria-hidden`). Pass `label` (translated) if the icon alone carries the meaning.
- `Icon.names` lists the set. In-house, provisional drawing, to be replaced by a designed set once the brand is set.
- Directional icons (arrows, chevrons) mirror in right-to-left layouts if an RTL locale is ever added. EFIGS is all left-to-right.

```ts
interface IconProps {
  name: IconName;
  size?: number;
  strokeWidth?: number;
  label?: string;
  className?: string
}
```

## Card

A container for a single subject: `surface`, `border` outline, `radius-lg`, padding `space-4`.

- Optional `title` (heading style) and `action` (a discreet "See all" link).
- `inverse`: `surface-inverse` background, for a single hero card per screen (current account, bank card).
- Gap between cards: `space-4` on mobile, `space-6` on desktop. No card inside a card.

```ts
interface CardProps {
  title?: React.ReactNode;
  action?: string;
  onAction?: () => void;
  inverse?: boolean;
  style?: React.CSSProperties;
  className?: string;
  children?: React.ReactNode
}
```

## SegmentedControl

An exclusive choice between 2 to 4 views of the same data (period, type).

- `options`: strings or `{value, label}`. Use `value`/`onChange` to control it, `defaultValue` otherwise.
- `role="radiogroup"`. Provide `label`. Beyond 4 options, or when translated labels no longer fit, use a menu.

```ts
interface SegmentedControlProps {
  options: Array<string | { value: string; label: string }>;
  value?: string;
  defaultValue?: string;
  onChange?: (v: string) => void;
  label?: string
}
```

## AmountField

Entry of an amount (default) or of text (`variant="text"`): budget limits, savings goals, manual entries.

- Amount: 56 px high, digits in `amount-lg` aligned right, `inputMode="decimal"`, currency in `ink-muted`.
- Accepts the active locale's decimal and grouping separators (`1,240.50` in en, `1 240,50` in fr, `1.240,50` in de).
- `hint` under the field (e.g. last month's spending in that category). `error` replaces it, in `negative`, and says how to fix it.
- The consumer owns the value (`value`/`onChange`) and the locale-aware parsing.

```ts
interface AmountFieldProps {
  label?: string;
  variant?: 'amount'|'text';
  value?: string;
  defaultValue?: string;
  onChange?: React.ChangeEventHandler<HTMLInputElement>;
  placeholder?: string;
  currency?: string;
  hint?: string;
  error?: string;
  icon?: IconName;
  style?: React.CSSProperties
}
```

## TransactionRow

One transaction in a list: icon tile, label, metadata, amount aligned right.

- Negative `amount` = expense in `ink`. Positive = `+` in `positive`. `alert` for fees and penalties, in `negative`.
- `pending`: label and amount in `ink-muted`, metadata "Pending".
- `category` as a neutral badge. `onClick` makes the row interactive (button, `surface-sunken` hover).
- Merchant labels are cleaned by the consumer ("Carrefour Market", not "CB CARREFOUR 0211"). Merchant names are bank data: displayed as-is, never translated.

```ts
interface TransactionRowProps {
  label: string;
  amount: number;
  icon?: IconName;
  meta?: string;
  category?: string;
  note?: string;
  pending?: boolean;
  alert?: boolean;
  onClick?: () => void
}
```

## BalanceCard

The home hero card: over-title, `xl` balance, signed delta, actions, sync freshness.

- Built-in eye for discreet mode (hides balance and delta).
- `delta` → positive/negative badge with an arrow. `period` as a label ("vs October").
- `actions`: at most two buttons (primary + secondary).
- `syncedAt` is always set: it is the proof that the number is current.
- `inverse` for the card of a specific account.

```ts
interface BalanceCardProps {
  amount: number;
  label?: string;
  delta?: number;
  period?: string;
  actions?: React.ReactNode;
  syncedAt?: string;
  hidden?: boolean;
  inverse?: boolean;
  style?: React.CSSProperties
}
```

## BudgetGauge

Progress of a category budget, with its state in words.

- < 80%: `highlight` gauge and "40% used".
- ≥ 80% (`warnAt`): `warning` and "Almost reached".
- > 100%: `negative`, full gauge and "Over by €22".
- What is left to spend and the days remaining (`daysLeft`) are more useful than the percentage: always show them.

```ts
interface BudgetGaugeProps {
  label: string;
  spent: number;
  limit: number;
  icon?: IconName;
  warnAt?: number;
  daysLeft?: number;
  style?: React.CSSProperties
}
```

## CategoryBar

Spending breakdown as a stacked bar + legend with values, preferred over a pie chart.

- 5 categories maximum, sorted descending, the last one always "Other" (`chart-5`).
- Colours `chart-1` … `chart-5`, separated by 2 px so they stay distinct even at low contrast (the lime is 1.3:1 on white: the legend carries the values).
- `aria-label` generated from the items, in the active locale.

```ts
interface CategoryBarProps {
  items: Array<{ label: string; value: number; color?: 1|2|3|4|5 }>;
  style?: React.CSSProperties
}
```

## Notice

A contextual message in the flow: upcoming direct debit, failed sync, goal reached.

- `tone`: `info`, `warning`, `negative` (announced with `role="alert"`), `positive`.
- A short bold title + one sentence saying what to do. `action` for a link.
- One notice per screen. No notice for a trivial confirmation (prefer a toast).

```ts
interface NoticeProps {
  tone?: 'info'|'warning'|'negative'|'positive';
  title?: string;
  action?: string;
  onAction?: () => void;
  children?: React.ReactNode
}
```

## TabBar

Main mobile navigation, 5 destinations maximum.

- Always icon + label. The active tab gets a `highlight` pill and `aria-current="page"`.
- Canonical order: Home, Transactions, Budgets, Savings, Profile.
- Labels are short in every EFIGS language. Check German, the longest, at 390 px.

```ts
interface TabBarProps {
  items: Array<{ value: string; label: string; icon: IconName }>;
  value?: string;
  defaultValue?: string;
  onChange?: (v: string) => void
}
```

`IconName` type: `plus`, `arrow-up-right`, `arrow-down-left`, `arrow-right-left`, `chevron-right`, `check`, `alert`, `info`, `eye`, `eye-off`, `home`, `chart`, `card`, `piggy`, `cart`, `building`, `briefcase`, `train`, `bolt`, `search`, `user`, `clock`.

Utility: `formatAmount(value, { locale, signed?, decimals?, currency })` → `Intl.NumberFormat(locale, { style: 'currency', currency })`, with the `-` sign replaced by `−` (U+2212), and `signDisplay: 'exceptZero'` when `signed`.


---

# Reference screen: Home (mobile, 390 px)

The reference home screen, assembled only from the components: greeting, balance card, alert, priority budget, recent transactions, tab bar.

Reading order: where I stand (balance) → what needs my attention (alert, budget) → what happened (recent).

Structure, top to bottom, gutter `space-4`, gap `space-4`:

1. Header: `title` "Hello {firstName}" + a `ghost` button with the `search` icon.
2. `BalanceCard`: balance, delta vs the previous month, "Sync now" (primary) and "Add account" (secondary) actions, sync freshness.
3. A warning `Notice` if a direct debit is due within 48 h.
4. `Card` "November budget" + "See all" action → `BudgetGauge` for the budget closest to its limit.
5. `Card` "Recent" + "See all" → 3 `TransactionRow`.
6. `TabBar`: Home, Transactions, Budgets, Savings, Profile.

---

# To do (out of current scope)

- Transactions, Budgets, Savings and Profile screens.
- Components: modal sheet (`radius-xl`, `scrim`, `shadow-pop`), toast, account row, balance-over-time chart, empty state, loading skeleton, language picker.
- Logo and final icon set (the current set is provisional).
- Responsive desktop grid and breakpoints. Motion principles (durations, curves).
- Translation workflow: catalogue format, contributor tooling, and a German screenshot pass in CI.
