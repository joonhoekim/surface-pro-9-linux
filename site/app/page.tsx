// SPDX-License-Identifier: MIT
// Copyright (C) 2026 Joonhoe Kim
import Link from "next/link";
import { REPO_URL, githubUrl } from "@/lib/config";
import { findDocByFile, getDocs, requireRoute } from "@/lib/docs";
import { headingLink } from "@/lib/markdown";
import {
  RESULT_LABEL,
  RESULT_ORDER,
  VERDICT_LABEL,
  getHardware,
  getKernel,
  getTests,
  type Verdict,
} from "@/lib/facts";

const VERDICT_ORDER: Verdict[] = ["works", "caveat", "partial", "unverified", "broken"];

/** A card of the "In the repository" section: a rendered document, or a directory on GitHub. */
interface Card {
  name: string;
  text: string;
  /** Markdown file whose page the card opens; the build stops if it is gone */
  file?: string;
  /** directory opened on GitHub when there is no page for it */
  dir?: string;
}

export default function Home() {
  const hardware = getHardware();
  const tests = getTests();
  const kernel = getKernel();
  const docs = getDocs();

  const count = (k: Verdict) => hardware.filter((h) => h.kind === k).length;
  const working = count("works") + count("caveat");
  const research = docs.filter((d) => d.group === "gaps" || d.group === "control");

  const cards: Card[] = [
    {
      name: "kernel/",
      file: "kernel/README.md",
      text: `${kernel.patches} patches on stable v${kernel.base} and the linux-surface ${kernel.series} series, the config fragment, and where each patch comes from.`,
    },
    {
      name: "packaging/, dkms/",
      file: "packaging/README.md",
      text: "Packages for Ubuntu, Debian and Arch Linux: the platform files, the helper, the touch and pen daemon, the kernel patches as DKMS modules for linux-surface's kernel, and a patched libcamera.",
    },
    {
      name: "flake.nix, nix/",
      file: "docs/nixos.md",
      text: "The NixOS module (hardware.surface-pro-9.*) and packages for the kernel, the helper, the touch and pen daemon, the pen tools and a patched libcamera.",
    },
    {
      name: "userspace/platform/",
      file: "userspace/platform/README.md",
      text: "Platform files for any distribution, with an installer: touch preset, speaker and microphone processing, sensor rule, initramfs and kernel command line.",
    },
    {
      name: "helper/",
      file: "docs/helper.md",
      text: "sp9-helperd, sp9-ctl and the settings app: power limits per profile, smart charging, panel self refresh, Type Cover backlight, touch settings, Slim Pen 2 haptics and coupling, over D-Bus.",
    },
    {
      name: "helper/crates/sp9-digitizer/",
      file: "helper/crates/sp9-digitizer/README.md",
      text: "sp9-digitizerd: touch and pen processing for this device's digitizer, in Rust, from raw reports to input events.",
    },
    {
      name: "userspace/pen/",
      file: "userspace/pen/README.md",
      text: "Slim Pen 2 tools: connection and reconnection, speed feedback to the digitizer, haptics.",
    },
    {
      name: "userspace/iptsd/",
      file: "userspace/iptsd/README.md",
      text: "Pen patches for iptsd: position, tilt, contact, pressure and button decoding for this digitizer.",
    },
    {
      name: "userspace/libcamera/",
      file: "userspace/libcamera/README.md",
      text: "Patches for libcamera's simple pipeline and software ISP: exposure, gain limits, focus, tuning files for both sensors.",
    },
    {
      name: "tools/",
      file: "tools/README.md",
      text: "Collection and measurement tools for Linux and for Windows, where the baseline was read.",
    },
  ];

  return (
    <main id="main" className="home">
      <section className="hero" aria-labelledby="hero-title">
        <p className="eyebrow">
          Microsoft Surface Pro 9 · Intel · SKU <code>Surface_Pro_9_2038</code>
        </p>
        <h1 id="hero-title">Linux on the Surface Pro 9 (Intel)</h1>
        <p className="lead">
          Kernel patches, packages for Ubuntu, Debian and Arch Linux, a NixOS module, a device helper and a touch and pen
          daemon, on top of <a href="https://github.com/linux-surface/linux-surface">linux-surface</a>.
        </p>
        <div className="status-badge" role="note">
          <span className="status-dot" aria-hidden="true" />
          <div>
            <strong>Status: maintenance.</strong> Kept working on the author&apos;s device; no schedule for new kernels.
            Contributions welcome. Keeping Windows installed is recommended: Surface firmware updates come through
            Windows Update.
          </div>
        </div>
        <figure className="stack" aria-labelledby="stack-caption">
          <ol className="stack-layers">
            <li className="layer ours">
              <span className="layer-name">Desktop integration</span>
              <span className="layer-text">Ubuntu, Debian and Arch packages · NixOS module · platform files and installer for other distributions</span>
            </li>
            <li className="layer-row">
              <div className="layer ours">
                <span className="layer-name">Device helper</span>
                <span className="layer-text">sp9-helperd · sp9-ctl · settings app</span>
              </div>
              <div className="layer ours">
                <span className="layer-name">Touch and pen</span>
                <span className="layer-text">sp9-digitizerd · pen tools · iptsd patches</span>
              </div>
            </li>
            <li className="layer ours">
              <span className="layer-name">{kernel.patches} kernel patches</span>
              <span className="layer-text">
                {kernel.origins
                  ? `${kernel.origins.upstream} upstream backports · ${kernel.origins.pending} from pending pull requests · ${kernel.origins.local} written here`
                  : "backports, pending pull requests and patches written here; kernel/PROVENANCE.md lists each"}
              </span>
            </li>
            <li className="layer">
              <span className="layer-name">linux-surface {kernel.series} patch series</span>
              <span className="layer-text">fetched at a pinned commit when the kernel is built, not copied</span>
            </li>
            <li className="layer">
              <span className="layer-name">Linux v{kernel.base}</span>
              <span className="layer-text">stable</span>
            </li>
          </ol>
          <figcaption id="stack-caption">
            What is built, bottom to top. The highlighted layers are this repository.
          </figcaption>
        </figure>
        <div className="cta">
          <Link className="button primary" href="/docs/">
            Read the docs
          </Link>
          <Link className="button" href={requireRoute("docs/install.md")}>
            Install
          </Link>
          <Link className="button" href={requireRoute("docs/hardware-status.md")}>
            Hardware status
          </Link>
          <a className="button" href={REPO_URL}>
            Source on GitHub
          </a>
        </div>
      </section>

      <section aria-labelledby="facts-title" className="section">
        <h2 id="facts-title" className="section-title">
          At a glance
        </h2>
        <dl className="facts">
          <div className="fact">
            <dt>Kernel</dt>
            <dd>
              Linux <strong>{kernel.base}</strong> + linux-surface {kernel.series} + <strong>{kernel.patches}</strong>{" "}
              patches
            </dd>
          </div>
          <div className="fact">
            <dt>Hardware</dt>
            <dd>
              <strong>{working}</strong> of {hardware.length} listed parts work
              {count("caveat") > 0 && <>, {count("caveat")} of them with open items</>}
            </dd>
          </div>
          <div className="fact">
            <dt>Tests</dt>
            <dd>
              <strong>{tests.total}</strong> defined: {tests.totals.pass} pass, {tests.totals.partial} partial,{" "}
              {tests.totals.fail} fail, {tests.totals.unverified + tests.totals.norecord} unverified
            </dd>
          </div>
          <div className="fact">
            <dt>Scope</dt>
            <dd>
              This SKU only. Not the Surface Pro 9 5G; other Surface devices belong to{" "}
              <a href="https://github.com/linux-surface/linux-surface">linux-surface</a>
            </dd>
          </div>
        </dl>
      </section>

      <section aria-labelledby="hw-title" className="section">
        <div className="section-head">
          <h2 id="hw-title" className="section-title">
            Hardware
          </h2>
          <Link href={requireRoute("docs/hardware-status.md")}>Measurements for each part →</Link>
        </div>
        <p className="muted">
          From the <Link href={headingLink("README.md", /^hardware status$/i)}>README&apos;s status table</Link>.
        </p>
        <ul className="legend" aria-label="Legend">
          {VERDICT_ORDER.filter((k) => count(k) > 0).map((k) => (
            <li key={k}>
              <span className={`swatch v-${k}`} aria-hidden="true" />
              {VERDICT_LABEL[k]} ({count(k)})
            </li>
          ))}
        </ul>
        <ul className="hw-grid">
          {hardware.map((h) => (
            <li key={h.name} className={`hw-item v-${h.kind}-edge`}>
              <span className={`swatch v-${h.kind}`} aria-hidden="true" />
              <span className="hw-name">{h.name}</span>
              <span className="hw-verdict">
                <span className="sr-only">{VERDICT_LABEL[h.kind]}: </span>
                {h.verdict}
              </span>
            </li>
          ))}
        </ul>
      </section>

      <section aria-labelledby="tests-title" className="section">
        <div className="section-head">
          <h2 id="tests-title" className="section-title">
            Tests
          </h2>
          <Link href={requireRoute("docs/test-results.md")}>Every run →</Link>
        </div>
        <p className="muted">
          {tests.total} tests in the <Link href={requireRoute("docs/test-plan.md")}>test plan</Link>, each at its last
          recorded run. Unverified means not tested yet, not a failure; most are manual tests.
          {tests.updated && <> Results last changed {tests.updated}.</>}
        </p>
        <ul className="legend" aria-label="Legend">
          {RESULT_ORDER.filter((k) => tests.totals[k] > 0).map((k) => (
            <li key={k}>
              <span className={`swatch r-${k}`} aria-hidden="true" />
              {RESULT_LABEL[k]} ({tests.totals[k]})
            </li>
          ))}
        </ul>
        <div className="test-areas">
          {tests.areas.map((a) => (
            <div className="test-row" key={a.prefix}>
              <div className="test-row-head">
                <span className="test-name">
                  {a.title} <span className="test-prefix">{a.prefix}</span>
                </span>
                <span className="test-counts">
                  {RESULT_ORDER.filter((k) => a.counts[k] > 0)
                    .map((k) => `${a.counts[k]} ${RESULT_LABEL[k]}`)
                    .join(" · ")}
                </span>
              </div>
              <div className="bar" aria-hidden="true">
                {RESULT_ORDER.filter((k) => a.counts[k] > 0).map((k) => (
                  <span key={k} className={`seg r-${k}`} style={{ flexGrow: a.counts[k] }} />
                ))}
              </div>
            </div>
          ))}
        </div>
      </section>

      <section aria-labelledby="tree-title" className="section">
        <h2 id="tree-title" className="section-title">
          In the repository
        </h2>
        <ul className="tree">
          {cards.map((c) => {
            const inner = (
              <>
                <code>{c.name}</code>
                <span>{c.text}</span>
              </>
            );
            return (
              <li key={c.name}>
                {c.file ? <Link href={requireRoute(c.file)}>{inner}</Link> : <a href={githubUrl(c.dir!, true)}>{inner}</a>}
              </li>
            );
          })}
        </ul>
      </section>

      {research.length > 0 && (
        <section aria-labelledby="research-title" className="section">
          <h2 id="research-title" className="section-title">
            Research
          </h2>
          <p className="muted">
            What Windows does with each part of the device, read from the running system, and what it takes to do the
            same on Linux. Statements carry their evidence: measured, observed, inferred or unverified.
          </p>
          <div className="research">
            {(["gaps", "control"] as const).map((g) => {
              const list = research.filter((d) => d.group === g);
              if (list.length === 0) return null;
              return (
                <div key={g}>
                  <h3 className="research-title">
                    {g === "gaps" ? "Linux against the Windows baseline" : "How Windows controls it"}
                  </h3>
                  <ul className="research-list">
                    {list.map((d) => (
                      <li key={d.file}>
                        <Link href={d.route}>{d.navTitle}</Link>
                      </li>
                    ))}
                  </ul>
                </div>
              );
            })}
          </div>
        </section>
      )}

      <section aria-labelledby="take-part-title" className="section">
        <h2 id="take-part-title" className="section-title">
          Take part
        </h2>
        <p className="muted">
          A test from the plan run on another Surface Pro 9 is as useful as code.{" "}
          {findDocByFile("CONTRIBUTING.md") && (
            <>
              <Link href={requireRoute("CONTRIBUTING.md")}>How to contribute</Link>
              {findDocByFile("SECURITY.md") && (
                <>
                  {" "}
                  · <Link href={requireRoute("SECURITY.md")}>Security policy</Link>
                </>
              )}
              {" · "}
            </>
          )}
          <Link href={headingLink("README.md", /^licen[cs]es?$/i)}>Licences</Link>
        </p>
      </section>
    </main>
  );
}
