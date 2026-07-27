import { defineConfig } from "vitepress";

// Project site served at https://fbnlrz.github.io/Inari/
export default defineConfig({
  title: "Inari",
  description:
    "Per-app audio routing, capturable OBS mixes and a processed mic for PipeWire — plus SteelSeries Arctis/OLED/Aerox control and a Tokyo Night theme.",
  base: "/Inari/",
  lang: "en-US",
  cleanUrls: true,
  lastUpdated: true,
  appearance: "force-dark", // Tokyo Night is a dark theme

  head: [
    ["link", { rel: "icon", type: "image/svg+xml", href: "/Inari/logo.svg" }],
    ["meta", { name: "theme-color", content: "#7aa2f7" }],
    ["meta", { property: "og:title", content: "Inari" }],
    [
      "meta",
      {
        property: "og:description",
        content:
          "Linux audio mixer for PipeWire with SteelSeries headset, OLED and mouse control.",
      },
    ],
  ],

  themeConfig: {
    logo: "/logo.svg",
    siteTitle: "Inari",

    nav: [
      { text: "Home", link: "/" },
      { text: "Guide", link: "/guide/getting-started" },
      { text: "Features", link: "/features/mixer" },
      { text: "Reference", link: "/reference/hardware" },
      { text: "Troubleshooting", link: "/troubleshooting" },
      { text: "Changelog", link: "/changelog" },
    ],

    sidebar: [
      {
        text: "Guide",
        items: [
          { text: "Getting started", link: "/guide/getting-started" },
          { text: "Updating", link: "/guide/updating" },
          { text: "Building from source", link: "/guide/building" },
        ],
      },
      {
        text: "Features",
        items: [
          { text: "Audio mixer", link: "/features/mixer" },
          { text: "Headset", link: "/features/headset" },
          { text: "OLED display", link: "/features/oled" },
          { text: "Mouse", link: "/features/mouse" },
        ],
      },
      {
        text: "Reference",
        items: [
          { text: "Supported hardware", link: "/reference/hardware" },
          { text: "Configuration & files", link: "/reference/configuration" },
          { text: "Protocols", link: "/reference/protocols" },
        ],
      },
      {
        text: "Project",
        items: [
          { text: "Troubleshooting", link: "/troubleshooting" },
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
