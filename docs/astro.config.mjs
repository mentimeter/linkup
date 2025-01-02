// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// https://astro.build/config
export default defineConfig({
	integrations: [
		starlight({
			title: 'Linkup Docs',
			social: {
				github: 'https://github.com/mentimeter/linkup',
			},
			sidebar: [
				{
					label: 'Deploy',
					autogenerate: { directory: 'deploy' },
				},
				{
					label: 'Install',
					autogenerate: { directory: 'install' },
				},
				{
					label: 'Run',
					autogenerate: { directory: 'run' },
				},
			],
		}),
	],
});
