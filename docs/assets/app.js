(function () {
  "use strict";

  const root = document.documentElement;
  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  /* ---------- 主题切换 ---------- */
  const themeBtn = document.querySelector(".theme-toggle");
  const themeMeta = document.querySelector('meta[name="theme-color"]');

  function syncTheme() {
    const dark = root.dataset.theme !== "light";
    if (themeMeta) {
      themeMeta.setAttribute("content", dark ? "#080a0f" : "#f7f8fc");
    }
    if (themeBtn) {
      const label = dark ? "切换到浅色主题" : "切换到深色主题";
      themeBtn.setAttribute("aria-label", label);
      themeBtn.setAttribute("title", label);
    }
  }

  if (themeBtn) {
    themeBtn.addEventListener("click", function () {
      const next = root.dataset.theme === "light" ? "dark" : "light";
      root.dataset.theme = next;
      try {
        localStorage.setItem("dp-theme", next);
      } catch (e) {
        /* 忽略隐私模式下的写入失败 */
      }
      syncTheme();
    });
    syncTheme();
  }

  /* ---------- 顶栏滚动状态 ---------- */
  const header = document.querySelector(".site-header");
  if (header) {
    const onScroll = function () {
      header.classList.toggle("is-scrolled", window.scrollY > 6);
    };
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
  }

  /* ---------- 滚动渐显 ---------- */
  const reveals = Array.prototype.slice.call(document.querySelectorAll(".reveal"));
  if ("IntersectionObserver" in window && !reduceMotion) {
    const io = new IntersectionObserver(
      function (entries) {
        entries.forEach(function (entry) {
          if (entry.isIntersecting) {
            entry.target.classList.add("is-in");
            io.unobserve(entry.target);
          }
        });
      },
      { rootMargin: "0px 0px -8% 0px", threshold: 0.12 }
    );
    reveals.forEach(function (el) {
      io.observe(el);
    });
  } else {
    reveals.forEach(function (el) {
      el.classList.add("is-in");
    });
  }

  /* ---------- 面板示意：切换功能 ---------- */
  const mock = document.getElementById("mock");
  const mockTitle = document.getElementById("mockTitle");
  const mockList = document.getElementById("mockList");
  const mockBody = mock ? mock.querySelector(".mock-body") : null;
  const mockTabs = Array.prototype.slice.call(document.querySelectorAll(".mock-tab"));

  const MOCK_TABS = {
    launcher: {
      title: "应用启动器",
      accent: "#4ba3ff",
      items: [
        ["VS Code", "应用"],
        ["github.com", "网址"],
        ["npm run tauri dev", "命令"],
      ],
    },
    notes: {
      title: "便签",
      accent: "#f59e0b",
      items: [
        ["明天 10 点评审", "今天"],
        ["买咖啡豆", "昨天"],
        ["周报草稿", "周一"],
      ],
    },
    pomodoro: {
      title: "番茄钟",
      accent: "#fb7185",
      items: [
        ["专注 25:00", "进行中"],
        ["短休 05:00", "待开始"],
        ["今日完成 4 轮", "统计"],
      ],
    },
    clipboard: {
      title: "剪贴板历史",
      accent: "#2dd4bf",
      items: [
        ["https://tauri.app", "链接"],
        ["npm run tauri dev", "命令"],
        ["设计稿 v3", "文本"],
      ],
    },
    toolbox: {
      title: "工具箱",
      accent: "#a78bfa",
      items: [
        ["二维码生成", "内置"],
        ["二维码解析", "内置"],
        ["新增工具", "三步接入"],
      ],
    },
    quick: {
      title: "快捷操作",
      accent: "#34d399",
      items: [
        ["深色模式", "切换"],
        ["锁屏", "系统"],
        ["系统设置", "直达"],
      ],
    },
    account: {
      title: "账户",
      accent: "#60a5fa",
      items: [
        ["手机号登录", "账户"],
        ["滑块人机验证", "安全"],
        ["Token 仅存本地", "隐私"],
      ],
    },
    settings: {
      title: "设置",
      accent: "#94a3b8",
      items: [
        ["跟随系统主题", "外观"],
        ["开机自启", "通用"],
        ["自动检查更新", "关于"],
      ],
    },
  };

  function renderMock(key) {
    const data = MOCK_TABS[key];
    if (!data || !mock) {
      return;
    }
    mock.style.setProperty("--mock-accent", data.accent);
    if (mockTitle) {
      mockTitle.textContent = data.title;
    }
    if (mockList) {
      const frag = document.createDocumentFragment();
      data.items.forEach(function (item) {
        const li = document.createElement("li");
        const label = document.createElement("span");
        const meta = document.createElement("em");
        label.textContent = item[0];
        meta.textContent = item[1];
        li.append(label, meta);
        frag.appendChild(li);
      });
      mockList.replaceChildren(frag);
    }
    mockTabs.forEach(function (btn) {
      const active = btn.dataset.tab === key;
      btn.classList.toggle("is-active", active);
      btn.setAttribute("aria-pressed", String(active));
    });
    if (!reduceMotion && mockBody && typeof mockBody.animate === "function") {
      mockBody.animate(
        [
          { opacity: 0.4, transform: "translateY(3px)" },
          { opacity: 1, transform: "none" },
        ],
        { duration: 180, easing: "ease-out" }
      );
    }
  }

  mockTabs.forEach(function (btn) {
    btn.addEventListener("click", function () {
      renderMock(btn.dataset.tab);
    });
  });

  if (mockTabs.length > 0) {
    renderMock("launcher");
  }

  /* ---------- 复制构建命令 ---------- */
  const BUILD_COMMANDS = "npm install\nnpm run tauri dev\nnpm run tauri build";
  const copyBtn = document.getElementById("copyBuild");
  let copyTimer = 0;

  function fallbackCopy(text) {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    ta.style.position = "fixed";
    ta.style.top = "-9999px";
    document.body.appendChild(ta);
    ta.select();
    let ok = false;
    try {
      ok = document.execCommand("copy");
    } catch (e) {
      ok = false;
    }
    ta.remove();
    return ok;
  }

  function flashCopied(ok) {
    if (!copyBtn) {
      return;
    }
    const label = copyBtn.querySelector("span");
    copyBtn.classList.toggle("is-copied", ok);
    if (label) {
      label.textContent = ok ? "已复制" : "复制失败";
    }
    window.clearTimeout(copyTimer);
    copyTimer = window.setTimeout(function () {
      copyBtn.classList.remove("is-copied");
      if (label) {
        label.textContent = "复制";
      }
    }, 1800);
  }

  if (copyBtn) {
    copyBtn.addEventListener("click", function () {
      if (navigator.clipboard && window.isSecureContext) {
        navigator.clipboard.writeText(BUILD_COMMANDS).then(
          function () {
            flashCopied(true);
          },
          function () {
            flashCopied(fallbackCopy(BUILD_COMMANDS));
          }
        );
      } else {
        flashCopied(fallbackCopy(BUILD_COMMANDS));
      }
    });
  }

  /* ---------- 移动端折叠菜单 ---------- */
  const navToggle = document.querySelector(".nav-toggle");
  const mobileNav = document.getElementById("mobileNav");
  const navMq = window.matchMedia("(max-width: 960px)");

  function setNavOpen(open) {
    if (!navToggle || !mobileNav) {
      return;
    }
    mobileNav.classList.toggle("is-open", open);
    navToggle.setAttribute("aria-expanded", String(open));
    navToggle.setAttribute("aria-label", open ? "关闭导航菜单" : "打开导航菜单");
  }

  function closeNav() {
    setNavOpen(false);
  }

  if (navToggle && mobileNav) {
    navToggle.addEventListener("click", function () {
      setNavOpen(!mobileNav.classList.contains("is-open"));
    });

    mobileNav.querySelectorAll("a").forEach(function (link) {
      link.addEventListener("click", closeNav);
    });

    document.addEventListener("keydown", function (event) {
      if (event.key === "Escape") {
        closeNav();
      }
    });

    document.addEventListener("click", function (event) {
      if (!mobileNav.classList.contains("is-open")) {
        return;
      }
      if (!mobileNav.contains(event.target) && !navToggle.contains(event.target)) {
        closeNav();
      }
    });

    const onNavBreakpoint = function () {
      if (!navMq.matches) {
        closeNav();
      }
    };
    if (typeof navMq.addEventListener === "function") {
      navMq.addEventListener("change", onNavBreakpoint);
    }
  }
})();
