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
				{ tag: 'style', content: ".kofi-fixed{position:fixed;left:1.25rem;bottom:1.25rem;z-index:9999;display:inline-flex;align-items:center;gap:0.5rem;font-weight:600;font-size:0.875rem;text-decoration:none;padding:0.6rem 1rem;background:#80CBC4;color:#10201d;border-radius:0.5rem;box-shadow:0 4px 14px rgba(0,0,0,0.4);transition:transform .15s ease,box-shadow .15s ease;}.kofi-fixed:hover{transform:translateY(-2px);box-shadow:0 8px 20px rgba(0,0,0,0.5);}@media(max-width:520px){.kofi-fixed{font-size:0.8rem;padding:0.5rem 0.8rem;left:0.8rem;bottom:0.8rem;}}" },
				{ tag: 'script', content: "(function(){function add(){if(document.body&&!document.querySelector('.kofi-fixed')){var a=document.createElement('a');a.className='kofi-fixed';a.href='https://ko-fi.com/george_builds';a.target='_blank';a.rel='noopener';a.textContent='\\u2615 Buy me a coffee';document.body.appendChild(a);}}document.addEventListener('astro:page-load',add);if(document.readyState!=='loading'){add();}else{document.addEventListener('DOMContentLoaded',add);}})();" },
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
