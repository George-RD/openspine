// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// https://astro.build/config
export default defineConfig({
	site: 'https://george-rd.github.io',
	base: '/openspine',
	integrations: [
		starlight({
			title: 'OpenSpine',
			tagline: 'The backbone for governed agents.',
			head: [
				{ tag: 'script', attrs: { src: 'https://storage.ko-fi.com/cdn/scripts/overlay-widget.js' } },
				{ tag: 'script', content: "kofiWidgetOverlay.draw('george_builds', { 'type': 'floating-chat', 'floating-chat.donateButton.text': 'Buy me a coffee ☕', 'floating-chat.donateButton.background-color': '#80CBC4', 'floating-chat.donateButton.text-color': '#000' });" },
			],
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
