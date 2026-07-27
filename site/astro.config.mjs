// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
	site: 'https://george-rd.github.io',
	base: '/openspine',
	integrations: [
		starlight({
			title: 'OpenSpine',
			tagline: 'Runtime-enforced authority for agents.',
			favicon: '/openspine/favicon.svg',
			customCss: ['./src/styles/starlight.css'],
			social: [
				{ icon: 'github', label: 'GitHub', href: 'https://github.com/George-RD/openspine' },
			],
			sidebar: [
				{ label: 'Why OpenSpine', slug: 'why-openspine' },
				{ label: 'Quickstart', slug: 'quickstart' },
				{ label: 'Architecture', slug: 'architecture' },
				{ label: 'Threat model', slug: 'threat-model' },
				{ label: 'Decisions', slug: 'decisions' },
				{ label: 'Roadmap', slug: 'roadmap' },
			],
		}),
	],
});
