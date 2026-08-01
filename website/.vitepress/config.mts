import { defineConfig } from "vitepress";

// Project site served at https://fbnlrz.github.io/Inari/
const SITE = "https://fbnlrz.github.io/Inari/";

export default defineConfig({
  title: "Inari",
  // "Inari" alone is ambiguous (a deity, a dish, a Finnish municipality); the
  // suffix is what makes a search result identifiable.
  titleTemplate: ":title | Inari — Linux audio mixer for PipeWire",
  description:
    "Per-app audio routing, capturable OBS mixes and a processed mic for PipeWire — plus SteelSeries Arctis/OLED/Aerox control and a Tokyo Night theme.",
  base: "/Inari/",
  lang: "en-US",
  cleanUrls: true,
  lastUpdated: true,
  appearance: "force-dark", // Tokyo Night is a dark theme

  // Absolute URL, needed for og:image and the sitemap: link previews and
  // crawlers can't resolve a relative path.
  sitemap: { hostname: SITE },

  head: [
    ["link", { rel: "icon", type: "image/svg+xml", href: "/Inari/logo.svg" }],
    ["meta", { name: "theme-color", content: "#7aa2f7" }],
    ["meta", { property: "og:site_name", content: "Inari" }],
    ["meta", { property: "og:type", content: "website" }],
    ["meta", { property: "og:url", content: SITE }],
    ["meta", { property: "og:title", content: "Inari — SteelSeries Sonar for Linux" }],
    [
      "meta",
      {
        property: "og:description",
        content:
          "Linux audio mixer for PipeWire with SteelSeries headset, OLED and mouse control.",
      },
    ],
    // Without this, every link shared to Discord/Reddit/Mastodon renders as a
    // bare text stub. The mixer shot is the one image that explains the app.
    ["meta", { property: "og:image", content: `${SITE}mixer.png` }],
    ["meta", { property: "og:image:width", content: "1280" }],
    ["meta", { property: "og:image:height", content: "800" }],
    ["meta", { property: "og:image:alt", content: "The Inari mixer, routing apps to Game, Chat and Music channels" }],
    ["meta", { name: "twitter:card", content: "summary_large_image" }],
    ["meta", { name: "twitter:title", content: "Inari — SteelSeries Sonar for Linux" }],
    [
      "meta",
      {
        name: "twitter:description",
        content:
          "Linux audio mixer for PipeWire with SteelSeries headset, OLED and mouse control.",
      },
    ],
    ["meta", { name: "twitter:image", content: `${SITE}mixer.png` }],
  ],

  themeConfig: {
    logo: "/logo.svg",
    siteTitle: "Inari",

    // Troubleshooting and the reference pages are mostly h3 sections; the
    // default (h2 only) leaves their outline nearly empty.
    outline: { level: [2, 3], label: "On this page" },

    nav: [
      { text: "Home", link: "/" },
      { text: "Guide", link: "/guide/getting-started" },
      { text: "Features", link: "/features/mixer" },
      { text: "Reference", link: "/reference/hardware" },
      { text: "Help", link: "/faq" },
      { text: "Changelog", link: "/changelog" },
    ],

    sidebar: [
      {
        text: "Guide",
        items: [
          { text: "Getting started", link: "/guide/getting-started" },
          { text: "First steps", link: "/guide/first-steps" },
          { text: "Remote", link: "/guide/remote" },
          { text: "Updating", link: "/guide/updating" },
          { text: "Building from source", link: "/guide/building" },
        ],
      },
      {
        text: "Features",
        items: [
          { text: "Mixer", link: "/features/mixer" },
          { text: "Mixes", link: "/features/mixes" },
          { text: "Equalizer", link: "/features/eq" },
          { text: "Microphone", link: "/features/mic" },
          { text: "Media", link: "/features/media" },
          { text: "Profiles", link: "/features/profiles" },
          { text: "Settings", link: "/features/settings" },
          { text: "Hotkeys", link: "/features/hotkeys" },
          { text: "Headset", link: "/features/headset" },
          { text: "OLED display", link: "/features/oled" },
          { text: "Mouse", link: "/features/mouse" },
          { text: "Keyboard", link: "/features/keyboard" },
        ],
      },
      {
        text: "Reference",
        items: [
          { text: "Supported hardware", link: "/reference/hardware" },
          { text: "Configuration & files", link: "/reference/configuration" },
          { text: "Command line", link: "/reference/cli" },
          { text: "Protocols", link: "/reference/protocols" },
        ],
      },
      // Help is what a user reaches for; Project is meta. Keeping them apart
      // stops "Troubleshooting" from being filed next to "Contributing".
      {
        text: "Help",
        items: [
          { text: "FAQ", link: "/faq" },
          { text: "Troubleshooting", link: "/troubleshooting" },
        ],
      },
      {
        text: "Project",
        items: [
          { text: "Contributing", link: "/contributing" },
          { text: "Changelog", link: "/changelog" },
        ],
      },
    ],

    socialLinks: [{ icon: "github", link: "https://github.com/fbnlrz/Inari" }],

    editLink: {
      pattern: "https://github.com/fbnlrz/Inari/edit/main/website/:path",
      text: "Edit this page on GitHub",
    },

    search: { provider: "local" },

    footer: {
      message:
        'Released under the GPL-3.0 License. A fork of <a href="https://github.com/NC1107/sink">NC1107/sink</a>.',
      copyright: "Not affiliated with or endorsed by SteelSeries.",
    },
  },
});
