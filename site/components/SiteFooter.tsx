// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
import Link from "next/link";
import { REPO_URL, githubUrl, withBase } from "@/lib/config";
import { findDocByFile } from "@/lib/docs";

export default function SiteFooter() {
  const names = findDocByFile("TRADEMARKS.md");
  return (
    <footer className="site-footer">
      <p>
        Documentation text is <a href={githubUrl("LICENSES/CC-BY-SA-4.0.txt")}>CC BY-SA 4.0</a>; code and commands
        quoted in it may also be used under MIT. Code is licensed per file: kernel patches GPL-2.0-only, the device
        helper and the touch and pen daemon (<code>helper/</code>) GPL-3.0-or-later, the rest MIT unless a file says
        otherwise; see <a href={githubUrl("LICENSE")}>LICENSE</a> and <a href={githubUrl("NOTICE")}>NOTICE</a>. Source
        on <a href={REPO_URL}>GitHub</a>. This site is built with open-source packages:{" "}
        <a href={withBase("/third-party-licenses.txt")}>third-party licenses</a>.
      </p>
      <p>
        An independent project, not affiliated with, sponsored by or endorsed by Microsoft, Intel, Realtek or the
        linux-surface project. Product names are trademarks of their owners and are used only to identify the hardware
        and software this work is for
        {names ? (
          <>
            ; see the <Link href={names.route}>name policy</Link>.
          </>
        ) : (
          "."
        )}
      </p>
    </footer>
  );
}
