/* ─── crosshair follower ─── */
(() => {
  const r = document.getElementById("reticle");
  if (!r) return;
  let raf: number | null = null;
  let x = 0;
  let y = 0;
  window.addEventListener(
    "mousemove",
    (e) => {
      x = e.clientX;
      y = e.clientY;
      if (!raf) {
        raf = requestAnimationFrame(() => {
          r.style.transform = `translate(${x}px, ${y}px) translate(-50%,-50%)`;
          raf = null;
        });
      }
    },
    { passive: true },
  );
})();

/* ─── scroll-in reveal ─── */
(() => {
  const els = document.querySelectorAll(".appear");
  if (!("IntersectionObserver" in window)) {
    els.forEach((e) => e.classList.add("in"));
    return;
  }
  const io = new IntersectionObserver(
    (entries) => {
      entries.forEach((e) => {
        if (e.isIntersecting) {
          e.target.classList.add("in");
          io.unobserve(e.target);
        }
      });
    },
    { rootMargin: "0px 0px -80px 0px", threshold: 0.05 },
  );
  els.forEach((e) => io.observe(e));
})();

/* ─── pause forever-animations when off-screen (saves compositor work) ─── */
(() => {
  if (!("IntersectionObserver" in window)) return;
  const watch = (sel: string) => {
    document.querySelectorAll(sel).forEach((el) => {
      const io = new IntersectionObserver(
        (entries) => {
          entries.forEach((e) =>
            e.target.classList.toggle("idle", !e.isIntersecting),
          );
        },
        { rootMargin: "120px" },
      );
      io.observe(el);
    });
  };
  watch(".gear-mark");
  const term = document.getElementById("term");
  if (term) {
    const io = new IntersectionObserver(
      (entries) => {
        entries.forEach((e) => {
          const cur = document.getElementById("cursor");
          if (cur) cur.classList.toggle("idle", !e.isIntersecting);
        });
      },
      { rootMargin: "120px" },
    );
    io.observe(term);
  }
})();

/* ─── typed terminal ─── */
(() => {
  const el = document.getElementById("term");
  if (!el) return;
  runTerminalDemo(el);
})();

function runTerminalDemo(termBody: HTMLElement) {
  const lines = [
    {
      t: 80,
      s: '<span class="p">~/repos/target-repo</span> <span class="d">$</span> <span class="c">scripts/jig doctor --summary</span>',
    },
    {
      t: 380,
      s: '<span class="d">jig doctor · checking environment ............ rev 0.1.x</span>',
    },
    {
      t: 240,
      s: '  <span class="ok">[ok]</span>  <span class="c">cargo</span>     <span class="d">1.85.0</span>',
    },
    {
      t: 200,
      s: '  <span class="ok">[ok]</span>  <span class="c">sqlx-cli</span>  <span class="d">0.7.4</span>',
    },
    {
      t: 200,
      s: '  <span class="ok">[ok]</span>  <span class="c">bun</span>       <span class="d">1.1.18</span>',
    },
    {
      t: 240,
      s: '  <span class="ok">[ok]</span>  <span class="c">.jig.toml</span> <span class="d">found · template_source pinned · github.com/bpcakes/jig-sh @ v0.1.x</span>',
    },
    {
      t: 240,
      s: '  <span class="er">[··]</span>  <span class="c">codex skills</span>  <span class="d">missing 2 (jig.gate / jig.receipts)</span>',
    },
    {
      t: 320,
      s: '<span class="d">→ run <span class="h">scripts/jig agent bootstrap</span> to install missing skills.</span>',
    },
    { t: 600, s: "" },
    {
      t: 0,
      s: '<span class="p">~/repos/target-repo</span> <span class="d">$</span> <span class="c">scripts/jig agent bootstrap</span>',
    },
    {
      t: 380,
      s: '<span class="d">bootstrap · resolving marketplace ............ bpcakes/jig-skills</span>',
    },
    {
      t: 260,
      s: '  <span class="ok">+</span> install <span class="c">jig.gate</span>      <span class="d">v0.3.2</span>',
    },
    {
      t: 260,
      s: '  <span class="ok">+</span> install <span class="c">jig.receipts</span>  <span class="d">v0.2.1</span>',
    },
    {
      t: 380,
      s: '<span class="ok">✓</span> bootstrap complete. <span class="d">2 skills installed · 0 errors</span>',
    },
    { t: 600, s: "" },
    {
      t: 0,
      s: '<span class="p">~/repos/target-repo</span> <span class="d">$</span> <span class="c">scripts/jig check</span>',
    },
    {
      t: 320,
      s: '<span class="d">→ scripts/jig check fmt       ............ </span><span class="ok">ok</span>',
    },
    {
      t: 320,
      s: '<span class="d">→ scripts/jig check clippy    ............ </span><span class="ok">ok</span>  <span class="d">(0 warn)</span>',
    },
    {
      t: 480,
      s: '<span class="d">→ scripts/jig check test      ............ </span><span class="ok">ok</span>  <span class="d">(184 passed · 12.4s)</span>',
    },
    {
      t: 320,
      s: '<span class="d">→ scripts/jig check contract  ............ </span><span class="ok">ok</span>  <span class="d">(.agent/jig-contract.json ≡ runtime)</span>',
    },
    {
      t: 320,
      s: '<span class="d">→ scripts/jig check agent-map ............ </span><span class="ok">ok</span>',
    },
    {
      t: 480,
      s: '<span class="ok">●</span> ci green · <span class="d">receipts written to .agent/state/receipts.jsonl</span>',
    },
  ];

  let i = 0;
  function next() {
    if (i >= lines.length) {
      termBody.insertAdjacentHTML(
        "beforeend",
        '<span class="p">~/repos/target-repo</span> <span class="d">$</span> <span id="cursor"></span>',
      );
      return;
    }
    const { t, s } = lines[i++];
    setTimeout(() => {
      if (s === "") termBody.insertAdjacentHTML("beforeend", "\n");
      else termBody.insertAdjacentHTML("beforeend", s + "\n");
      next();
    }, t);
  }
  const start = () => {
    if (termBody.dataset.started) return;
    termBody.dataset.started = "1";
    next();
  };
  if ("IntersectionObserver" in window) {
    const io = new IntersectionObserver(
      (es) => {
        if (es.some((e) => e.isIntersecting)) {
          start();
          io.disconnect();
        }
      },
      { threshold: 0.25 },
    );
    io.observe(termBody);
  } else {
    start();
  }
}
