import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(here, '..');
const read = (relativePath) => readFile(path.join(siteRoot, relativePath), 'utf8');

test('the landing page renders the dual-audience explainer in the first viewport', async () => {
	const page = await read('src/pages/index.astro');
	assert.match(page, /import AudienceExplainer from ['"]\.\.\/components\/AudienceExplainer\.astro['"]/);
	assert.match(page, /<AudienceExplainer\s*\/>/);
});

test('the explainer offers keyboard-addressable business and technical views', async () => {
	const component = await read('src/components/AudienceExplainer.astro');
	assert.match(component, /role="tablist"/);
	assert.match(component, /data-explainer-tab="business"/);
	assert.match(component, /data-explainer-tab="technical"/);
	assert.match(component, /aria-controls="explainer-business"/);
	assert.match(component, /aria-controls="explainer-technical"/);
	assert.match(component, /role="tabpanel"/);
	assert.match(component, /<AuthorityTrace\s*\/>/);
});

test('the business view explains a bounded job without architecture jargon', async () => {
	const component = await read('src/components/AudienceExplainer.astro');
	for (const phrase of [
		'One assistant holds the keyring',
		'This job gets a temporary pass',
		'Read 3 selected threads',
		'Create draft replies',
		'Send email',
		'When the task ends',
		'Proposal only',
		'Pause or revoke',
	]) {
		assert.ok(component.includes(phrase), `missing business explanation: ${phrase}`);
	}
});

test('the technical view names the enforced runtime boundary', async () => {
	const component = await read('src/components/AudienceExplainer.astro');
	for (const phrase of [
		'The model proposes effects',
		'The runtime owns authority',
		'Credentials stay outside the worker',
		'Explicit deny wins',
		'Every decision is recorded',
	]) {
		assert.ok(component.includes(phrase), `missing technical explanation: ${phrase}`);
	}
});

test('explainer motion remains useful with reduced motion and on small screens', async () => {
	const css = await read('src/styles/landing-explainer.css');
	assert.match(css, /@media \(max-width: 720px\)/);
	assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
	assert.match(css, /\.business-explainer\.is-running/);
	assert.match(css, /\[hidden\]/);
});

test('rendered-site QA exercises both explanations', async () => {
	const capture = await read('scripts/capture-site.cjs');
	assert.match(capture, /data-explainer-tab="technical"/);
	assert.match(capture, /landing-desktop-business/);
	assert.match(capture, /landing-desktop-technical/);
	assert.match(capture, /Business view/);
	assert.match(capture, /Technical view/);
});
