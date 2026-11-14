// SPDX-License-Identifier: AGPL-3.0-or-later
// The provisional Solde icon set: 1.75 stroke, 24 grid, round caps,
// `currentColor`. Decorative unless given a translated `label`.

const PATHS = {
  plus: 'M12 5v14M5 12h14',
  'arrow-up-right': 'M7 17 17 7M8 7h9v9',
  'arrow-down-left': 'M17 7 7 17M16 17H7V8',
  'chevron-right': 'm9 6 6 6-6 6',
  check: 'm5 12.5 4.5 4.5L19 7',
  alert: 'M12 3.5 2.5 20h19L12 3.5zM12 10v4.5M12 17.5v.01',
  info: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v5M12 8v.01',
  building: 'M4 21h16M6 21V9.5l6-4.5 6 4.5V21M10 13h4M10 17h4',
  user: 'M12 4a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM4 21c0-4 3.6-6 8-6s8 2 8 6',
  clock: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7v5l3 2',
  card: 'M5 5h14a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2zM3 10h18M7 15h4',
  piggy:
    'M5 11.5C5 8 8.1 5.5 12 5.5c2.6 0 4.8 1.1 6 2.8h1.5v4.2H18a6.6 6.6 0 0 1-2.5 2.9V19h-3v-1.6h-2V19h-3v-3.2A5.6 5.6 0 0 1 5 11.5zM15 10v.01',
  chart: 'M4 4v16h16M8 16v-5M12 16V8M16 16v-3',
  briefcase:
    'M5 7h14a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2zM9 7V5h6v2M3 13h18',
  bolt: 'M13 3 5 14h6l-1 7 8-11h-6l1-7z',
  eye: 'M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12zM12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z',
} as const;

export type IconName = keyof typeof PATHS;

interface IconProps {
  name: IconName;
  size?: number;
  strokeWidth?: number;
  /** Translated accessible name, when the icon alone carries meaning. */
  label?: string;
  className?: string;
}

export function Icon({
  name,
  size = 20,
  strokeWidth = 1.75,
  label,
  className,
}: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      role={label ? 'img' : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      focusable="false"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
