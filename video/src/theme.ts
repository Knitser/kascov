/* Design tokens ported from the kascov website (web/style.css). */

export const T = {
  bg: '#0a100f',
  bgSoft: '#0e1514',
  card: '#111a18',
  cardHover: '#15201e',
  border: 'rgba(255, 255, 255, 0.09)',
  borderStrong: 'rgba(112, 199, 186, 0.35)',
  text: '#e9f1ef',
  muted: '#a7b8b4',
  faint: '#8fa19d',
  accent: '#70c7ba',
  accentSoft: 'rgba(112, 199, 186, 0.12)',
  born: '#6fd9a4',
  bornSoft: 'rgba(111, 217, 164, 0.14)',
  move: '#98a8ff',
  moveSoft: 'rgba(152, 168, 255, 0.14)',
  burn: '#f2a566',
  burnSoft: 'rgba(242, 165, 102, 0.14)',
  mono: 'ui-monospace, SFMono-Regular, Menlo, "JetBrains Mono", monospace',
  sans: 'Inter, system-ui, -apple-system, "Segoe UI", sans-serif',
  margin: 80,
} as const;

export const KIND_COLOR: Record<string, string> = {
  genesis: T.born,
  transition: T.move,
  burn: T.burn,
};

export const KIND_SOFT: Record<string, string> = {
  genesis: T.bornSoft,
  transition: T.moveSoft,
  burn: T.burnSoft,
};
