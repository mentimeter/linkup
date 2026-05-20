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
      social: [
        { icon: 'github', label: 'GitHub', href: 'https://github.com/mentimeter/linkup' },
      ],
      sidebar: [
        {
          label: "Getting Started",
          items: [
            { label: "What is Linkup?", slug: "explanation/what-is-linkup" },
            { label: "Deploy to Cloudflare", slug: "guides/deploy-linkup" },
            { label: "Run a Local Session", slug: "guides/local-env" },
          ],
        },
        {
          label: "Sessions",
          items: [
            { label: "Managing Sessions", slug: "guides/sessions" },
            { label: "Preview Environments", slug: "guides/preview-env" },
          ],
        },
        {
          label: "Guides",
          items: [
            { label: "Configure Linkup", slug: "guides/configure" },
            { label: "Local DNS", slug: "guides/local-dns" },
            { label: "Updating and Uninstalling", slug: "guides/maintaining" },
            { label: "Troubleshooting", slug: "guides/troubleshooting" },
          ],
        },
        {
          label: "Concepts",
          items: [
            {
              label: "What does a setup look like?",
              slug: "explanation/what-does-a-setup-look-like",
            },
            { label: "How Linkup works", slug: "explanation/how-it-works" },
          ],
        },
        {
          label: "Reference",
          items: [
            { label: "Config Reference", slug: "reference/config" },
            { label: "Shell Completion", slug: "reference/shell-completion" },
            {
              label: "Cloudflare Resources",
              slug: "reference/cloudflare-resources",
            },
          ],
        },
      ],
      customCss: ["./src/styles/custom.css"],
    }),
  ],
});
