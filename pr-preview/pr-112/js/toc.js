(function () {
  const toc = document.querySelector(".docs-toc");

  if (!toc) {
    return;
  }

  const content = document.querySelector(".docs-content");
  const tocLinks = Array.from(toc.querySelectorAll("a[href]"));
  const tocGroups = Array.from(toc.querySelectorAll("[data-docs-toc-group]"));

  if (!content || tocLinks.length === 0) {
    return;
  }

  function normalizePath(path) {
    return path.replace(/\/index\.html$/, "/");
  }

  function decodeHash(hash) {
    if (!hash || hash === "#") {
      return "";
    }

    try {
      return decodeURIComponent(hash.slice(1));
    } catch (_) {
      return hash.slice(1);
    }
  }

  function getSamePageHashId(link) {
    try {
      const url = new URL(link.getAttribute("href"), window.location.href);

      if (normalizePath(url.pathname) !== normalizePath(window.location.pathname)) {
        return "";
      }

      return decodeHash(url.hash);
    } catch (_) {
      return "";
    }
  }

  const linkById = new Map();

  tocLinks.forEach(function (link) {
    const id = getSamePageHashId(link);

    if (!id) {
      return;
    }

    link.classList.add("toc-link");

    if (!linkById.has(id)) {
      linkById.set(id, link);
    }
  });

  const headings = Array.from(
    content.querySelectorAll("h1[id], h2[id], h3[id], h4[id], h5[id], h6[id]")
  ).filter(function (heading) {
    return linkById.has(heading.id);
  });

  if (headings.length === 0) {
    return;
  }

  let activeHeadingId = "";

  function setActiveHeading(id) {
    if (activeHeadingId === id) {
      return;
    }

    activeHeadingId = id;

    tocLinks.forEach(function (link) {
      link.classList.remove("is-active");
      link.removeAttribute("aria-current");
    });

    tocGroups.forEach(function (group) {
      group.open = false;
    });

    const activeLink = linkById.get(id);

    if (!activeLink) {
      return;
    }

    activeLink.classList.add("is-active");
    activeLink.setAttribute("aria-current", "location");

    let parentGroup = activeLink.closest("[data-docs-toc-group]");

    while (parentGroup) {
      parentGroup.open = true;
      parentGroup = parentGroup.parentElement.closest("[data-docs-toc-group]");
    }
  }

  function getHeaderOffset() {
    const header = document.querySelector(".site-header");

    if (!header) {
      return 96;
    }

    return Math.ceil(header.getBoundingClientRect().height + 16);
  }

  // VENDORED DIVERGENCE: short sections clustered at the page bottom can
  // never scroll their headings past the header line, so (a) the scroll spy
  // clamps to the LAST heading when the page is scrolled to the bottom, and
  // (b) an explicit hash (URL fragment or TOC click) pins its link as
  // active until the user actually scrolls, so position-based updates no
  // longer override a navigation the user just made.
  let hashOverrideId = "";

  function headingIdFromLocationHash() {
    const id = decodeHash(window.location.hash);

    return linkById.has(id) ? id : "";
  }

  function getCurrentHeading() {
    const doc = document.documentElement;

    if (window.scrollY + window.innerHeight >= doc.scrollHeight - 2) {
      return headings[headings.length - 1];
    }

    const offset = getHeaderOffset();
    let currentHeading = headings[0];

    headings.forEach(function (heading) {
      if (heading.getBoundingClientRect().top - offset <= 1) {
        currentHeading = heading;
      }
    });

    return currentHeading;
  }

  function updateActiveHeading() {
    if (hashOverrideId) {
      setActiveHeading(hashOverrideId);
      return;
    }

    setActiveHeading(getCurrentHeading().id);
  }

  hashOverrideId = headingIdFromLocationHash();
  setActiveHeading(headings[0].id);
  updateActiveHeading();

  tocLinks.forEach(function (link) {
    link.addEventListener("click", function () {
      const id = getSamePageHashId(link);

      if (id && linkById.has(id)) {
        hashOverrideId = id;
        setActiveHeading(id);
      }
    });
  });

  window.addEventListener("hashchange", function () {
    hashOverrideId = headingIdFromLocationHash();
    window.requestAnimationFrame(updateActiveHeading);
  });

  // A real user scroll releases the hash pin and hands control back to
  // the scroll spy. Wheel/touch/keys/scrollbar-drag are user intent; the
  // scroll event itself is not (the browser's own hash jump fires it).
  const SCROLL_KEYS = new Set([
    "ArrowUp",
    "ArrowDown",
    "PageUp",
    "PageDown",
    "Home",
    "End",
    " "
  ]);

  function releaseHashOverride(event) {
    if (event.type === "keydown" && !SCROLL_KEYS.has(event.key)) {
      return;
    }

    if (!hashOverrideId) {
      return;
    }

    hashOverrideId = "";
    window.requestAnimationFrame(updateActiveHeading);
  }

  window.addEventListener("wheel", releaseHashOverride, { passive: true });
  window.addEventListener("touchmove", releaseHashOverride, { passive: true });
  window.addEventListener("mousedown", releaseHashOverride);
  window.addEventListener("keydown", releaseHashOverride);
  // END VENDORED DIVERGENCE

  if (!("IntersectionObserver" in window)) {
    return;
  }

  const observer = new IntersectionObserver(
    function () {
      window.requestAnimationFrame(updateActiveHeading);
    },
    {
      rootMargin: `-${getHeaderOffset()}px 0px -70% 0px`,
      threshold: 0
    }
  );

  headings.forEach(function (heading) {
    observer.observe(heading);
  });
})();
