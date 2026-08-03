import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(here, '..');
const read = (relativePath) => readFile(path.join(siteRoot, relativePath), 'utf8');

test('the first viewport keeps the existing hero slot and replaces its dense trace with the audience explainer', async () => {
	const page = await read('src/pages/index.astro');
	assert.match(page, /<AuthorityTrace\s*\/>/);
	const trace = await read('src/components/AuthorityTrace.astro');
	assert.match(trace, /import AudienceExplainer from ['"]\.\/AudienceExplainer\.astro['"]/);
	assert.match(trace, /<AudienceExplainer\s*\/>/);
	assert.match(trace, /authority-trace--audience/);
});

test('the explainer offers keyboard-addressable business and technical views', async () => {
	const component = await read('src/components/AudienceExplainer.astro');
	assert.match(component, /role="tablist"/);
	assert.match(component, /data-explainer-tab="business"/);
	assert.match(component, /data-explainer-tab="technical"/);
	assert.match(component, /aria-controls="explainer-business"/);
	assert.match(component, /aria-controls="explainer-technical"/);
	assert.match(component, /role="tabpanel"/);
	assert.match(component, /ArrowRight/);
	assert.match(component, /ArrowLeft/);
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

test('the visual stays truthful about what is and is not shipped', async () => {
	const component = await read('src/components/AudienceExplainer.astro');
	assert.match(component, /current Gmail proof demonstrates the one-task boundary/);
	assert.match(component, /recurring-responsibility experience shown above is still being built/);
});

test('explainer motion remains useful with reduced motion and on small screens', async () => {
	const styles = await Promise.all([
		'landing-explainer-core.css',
		'landing-explainer-business.css',
		'landing-explainer-growth.css',
		'landing-explainer-technical.css',
		'landing-explainer-motion.css',
		'landing-explainer-responsive.css',
	].map((file) => read(`src/styles/${file}`)));
	const css = styles.join('\n');
	assert.match(css, /@media \(max-width: 720px\)/);
	assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
	assert.match(css, /\.business-explainer\.is-running/);
	assert.match(css, /\.technical-explainer\.is-running/);
	assert.match(css, /\[hidden\]/);
});

test('rendered-site QA exercises both explanations', async () => {
	const capture = await read('scripts/capture-explainers.cjs');
	assert.match(capture, /data-explainer-tab="technical"/);
	assert.match(capture, /landing-desktop-business/);
	assert.match(capture, /landing-desktop-technical/);
	assert.match(capture, /Business view/);
	assert.match(capture, /Technical view/);
});

test('the docs workflow executes the explainer-specific rendered QA', async () => {
	const workflow = await read('../.github/workflows/site-check.yml');
	assert.match(workflow, /node scripts\/capture-explainers\.cjs/);
});
