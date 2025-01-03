// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';


// https://astro.build/config
export default defineConfig({
	integrations: [
		starlight({
			title: 'Linkup',
			social: {
				github: 'https://github.com/mentimeter/linkup',
			},
			sidebar: [
				{
					label: 'How To Guides',
					autogenerate: { directory: 'how-to' },
				},
				{
					label: 'Tutorials',
					autogenerate: { directory: 'tutorials' },
				},
				{
					label: 'Explanations',
					autogenerate: { directory: 'explanation' },
				},
				{
					label: 'Reference',
					autogenerate: { directory: 'reference' },
				},
			],
			customCss: [
        './src/styles/custom.css',
      ],
		}),
	],
});
