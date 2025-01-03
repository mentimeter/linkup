// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

// https://astro.build/config
export default defineConfig({
  site: "https://mentimeter.github.io",
  base: "linkup",
  integrations: [
    starlight({
      title: "Linkup",
      social: {
        github: "https://github.com/mentimeter/linkup",
      },
      sidebar: [
        {
          label: "Explanations",
          autogenerate: { directory: "explanation" },
        },
        {
          label: "How To Guides",
          autogenerate: { directory: "guides" },
        },
        {
          label: "Tutorials",
          autogenerate: { directory: "tutorials" },
        },
        {
          label: "Reference",
          autogenerate: { directory: "reference" },
        },
      ],
      customCss: ["./src/styles/custom.css"],
    }),
  ],
});
