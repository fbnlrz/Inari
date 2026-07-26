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
      { text: "Devices", link: "/guide/devices" },
      {
        text: "v1.0",
        items: [
          { text: "Releases", link: "https://github.com/fbnlrz/Inari/releases" },
          { text: "Changelog", link: "https://github.com/fbnlrz/Inari/commits/main" },
        ],
      },
    ],

    sidebar: {
      "/guide/": [
        {
          text: "Getting started",
          items: [
            { text: "Install", link: "/guide/getting-started" },
            { text: "Updating", link: "/guide/updating" },
            { text: "Building from source", link: "/guide/building" },
          ],
        },
        {
          text: "Features",
          items: [
            { text: "Audio mixer", link: "/guide/mixer" },
            { text: "SteelSeries devices", link: "/guide/devices" },
          ],
        },
      ],
    },

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
