// Minimal stroke icons (24x24, currentColor)
import type { ReactNode } from "react";

const I = ({ children }: { children: ReactNode }) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">{children}</svg>
);

export const Ico = {
  models: <I><path d="M12 3 4 7.5v9L12 21l8-4.5v-9L12 3Z" /><path d="M4 7.5 12 12l8-4.5M12 12v9" /></I>,
  play: <I><path d="M6 4.5v15l13-7.5-13-7.5Z" /></I>,
  folder: <I><path d="M3 6.5A1.5 1.5 0 0 1 4.5 5h4.2l2 2.2h8.8A1.5 1.5 0 0 1 21 8.7v9.8a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5v-12Z" /></I>,
  server: <I><rect x="3.5" y="4" width="17" height="6.5" rx="1.5" /><rect x="3.5" y="13.5" width="17" height="6.5" rx="1.5" /><path d="M7 7.2h.01M7 16.7h.01M11 7.2h6M11 16.7h6" /></I>,
  gear: <I><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z" /></I>,
  plus: <I><path d="M12 5v14M5 12h14" /></I>,
  x: <I><path d="M6 6l12 12M18 6 6 18" /></I>,
  trash: <I><path d="M4 7h16M9 7V4.5h6V7M6.5 7l1 13h9l1-13" /></I>,
  copy: <I><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V5.5A1.5 1.5 0 0 0 14.5 4h-9A1.5 1.5 0 0 0 4 5.5v9A1.5 1.5 0 0 0 5.5 16H8" /></I>,
  download: <I><path d="M12 4v11M7 10.5l5 5 5-5M5 20h14" /></I>,
  pause: <I><path d="M8 5v14M16 5v14" /></I>,
  star: <I><path d="m12 3.5 2.6 5.4 5.9.8-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.8L12 3.5Z" /></I>,
  bolt: <I><path d="M13 3 5 13.5h6L10 21l8-10.5h-6L13 3Z" /></I>,
  chip: <I><rect x="6" y="6" width="12" height="12" rx="2" /><path d="M9 2.5V6M15 2.5V6M9 18v3.5M15 18v3.5M2.5 9H6M2.5 15H6M18 9h3.5M18 15h3.5" /></I>,
  search: <I><circle cx="11" cy="11" r="6.5" /><path d="m20 20-4.2-4.2" /></I>,
  refresh: <I><path d="M20 11a8 8 0 0 0-14.5-4.5L4 8M4 4v4h4M4 13a8 8 0 0 0 14.5 4.5L20 16M20 20v-4h-4" /></I>,
  compare: <I><path d="M8 4v16M16 4v16M4 8h8M12 16h8" /></I>,
  globe: <I><circle cx="12" cy="12" r="8.5" /><path d="M3.5 12h17M12 3.5c2.5 2.7 2.5 14.3 0 17M12 3.5c-2.5 2.7-2.5 14.3 0 17" /></I>,
  external: <I><path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></I>,
  sun: <I><circle cx="12" cy="12" r="4" /><path d="M12 2.5v2M12 19.5v2M4.6 4.6l1.4 1.4M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4 6 18M18 6l1.4-1.4" /></I>,
  moon: <I><path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5Z" /></I>,
  chart: <I><path d="M4 20V4M4 20h16M8 16v-5M12 16V8M16 16v-3" /></I>,
};

export function Logo({ size = 34, color = "var(--violet)" }: { size?: number; color?: string }) {
  return (
    <svg viewBox="-100 -100 200 200" width={size} height={size} aria-label="FERMI">
      <path d="M-65.82 -24L-3.46 12L-3.46 84L-65.82 48ZM3.46 12L65.82 -24L65.82 48L3.46 84Z" fill={color} stroke={color} strokeWidth="6" strokeLinejoin="round" />
      <path d="M0 -79.6L58.2 -46L0 -12.4L-58.2 -46Z" fill="none" stroke={color} strokeWidth="11" strokeLinejoin="round" />
    </svg>
  );
}
