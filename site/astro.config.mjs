// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// https://astro.build/config
export default defineConfig({
	integrations: [
		starlight({
			title: 'OpenSpine',
			tagline: 'The backbone for governed agents.',
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
