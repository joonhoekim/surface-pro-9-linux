// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim

// The project's mark: a slate on its kickstand with a shell prompt on the screen. Drawn for this
// project; app/icon.svg is the same drawing on a coloured square.
export default function Logo({ size = 24 }: { size?: number }) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" width={size} height={size} fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <rect x="2.5" y="4" width="19" height="13" rx="1.8" />
      <path d="M7 8.6l2.6 2-2.6 2M12 12.8h4.2" />
      <path d="M14.5 17l3 3.5M3 20.5h18" />
    </svg>
  );
}
