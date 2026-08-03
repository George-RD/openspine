const fs = require('node:fs/promises');
const path = require('node:path');
const { chromium } = require('playwright');

const siteUrl = process.env.SITE_URL || 'http://127.0.0.1:4321/openspine/';
const outputDir = path.resolve(process.cwd(), 'visual-artifacts');
const componentScreenshotStyle = '.skip-link { display: none !important; }';

async function inspectExplainer(page) {
	return page.locator('[data-audience-explainer]').evaluate((element) => {
		const selected = element.querySelector('[data-explainer-tab][aria-selected="true"]');
		const visiblePanel = element.querySelector('[data-explainer-panel]:not([hidden])');
		const rect = element.getBoundingClientRect();
		return {
			selectedText: selected?.textContent?.replace(/\s+/g, ' ').trim() || '',
			selectedView: selected?.getAttribute('data-explainer-tab') || '',
			visiblePanel: visiblePanel?.getAttribute('data-explainer-panel') || '',
			text: visiblePanel?.textContent?.replace(/\s+/g, ' ').trim() || '',
			width: Math.round(rect.width * 10) / 10,
			scrollWidth: element.scrollWidth,
			clientWidth: element.clientWidth,
			businessRunning: element.querySelector('.business-explainer')?.classList.contains('is-running') || false,
			technicalRunning: element.querySelector('.technical-explainer')?.classList.contains('is-running') || false,
		};
	});
}

async function createPage(browser, options) {
	const context = await browser.newContext({
		viewport: options.viewport,
		deviceScaleFactor: 1,
		colorScheme: 'light',
		reducedMotion: options.reducedMotion,
	});
	const page = await context.newPage();
	const errors = [];
	page.on('console', (message) => {
		if (message.type() === 'error' && !/fonts\.(googleapis|gstatic)\.com/.test(message.text())) {
			errors.push(`Console error: ${message.text()}`);
		}
	});
	page.on('pageerror', (error) => errors.push(`Page error: ${error.message}`));
	await page.goto(siteUrl, { waitUntil: 'networkidle', timeout: 60_000 });
	await page.waitForSelector('[data-audience-explainer]', { state: 'visible', timeout: 15_000 });
	await page.waitForTimeout(options.reducedMotion === 'reduce' ? 120 : 1_100);
	return { context, page, errors };
}

async function captureDesktop(browser) {
	const { context, page, errors } = await createPage(browser, {
		viewport: { width: 1440, height: 1000 },
		reducedMotion: 'no-preference',
	});
	const issues = errors;
	const explainer = page.locator('[data-audience-explainer]');

	await page.waitForTimeout(4_200);
	const business = await inspectExplainer(page);
	if (business.selectedView !== 'business') issues.push(`Business view was not selected: ${business.selectedView}`);
	if (!business.selectedText.includes('Business view')) issues.push('Business view tab label is missing');
	for (const phrase of ['temporary pass', '3 selected threads', 'Send email', 'Pause or revoke']) {
		if (!business.text.includes(phrase)) issues.push(`Business view is missing: ${phrase}`);
	}
	if (!business.businessRunning) issues.push('Business animation did not start');
	if (business.scrollWidth > business.clientWidth + 1) issues.push('Business explainer overflows horizontally');
	await explainer.screenshot({
		path: path.join(outputDir, 'landing-desktop-business.png'),
		style: componentScreenshotStyle,
	});

	const businessTab = page.locator('[data-explainer-tab="business"]');
	const technicalTab = page.locator('[data-explainer-tab="technical"]');
	await businessTab.focus();
	await page.keyboard.press('ArrowRight');
	if ((await technicalTab.getAttribute('aria-selected')) !== 'true') issues.push('ArrowRight did not activate Technical view');
	await page.keyboard.press('ArrowLeft');
	if ((await businessTab.getAttribute('aria-selected')) !== 'true') issues.push('ArrowLeft did not return to Business view');

	await technicalTab.click();
	await page.waitForSelector('[data-explainer-panel="technical"]', { state: 'visible' });
	await page.waitForTimeout(3_500);
	const technical = await inspectExplainer(page);
	if (technical.selectedView !== 'technical') issues.push(`Technical view was not selected: ${technical.selectedView}`);
	if (!technical.selectedText.includes('Technical view')) issues.push('Technical view tab label is missing');
	for (const phrase of ['runtime owns authority', 'Credentials stay outside the worker', 'Explicit deny wins', 'Every decision is recorded']) {
		if (!technical.text.includes(phrase)) issues.push(`Technical view is missing: ${phrase}`);
	}
	if (!technical.technicalRunning) issues.push('Technical animation did not start');
	if (technical.scrollWidth > technical.clientWidth + 1) issues.push('Technical explainer overflows horizontally');
	await explainer.screenshot({
		path: path.join(outputDir, 'landing-desktop-technical.png'),
		style: componentScreenshotStyle,
	});

	await context.close();
	return { business, technical, issues };
}

async function captureMobileReduced(browser) {
	const { context, page, errors } = await createPage(browser, {
		viewport: { width: 390, height: 844 },
		reducedMotion: 'reduce',
	});
	const explainer = page.locator('[data-audience-explainer]');
	const issues = errors;

	const business = await inspectExplainer(page);
	if (business.selectedView !== 'business') issues.push('Reduced-motion mobile did not open on Business view');
	if (business.businessRunning) issues.push('Reduced-motion business animation should remain static');
	if (business.scrollWidth > business.clientWidth + 1) issues.push('Reduced-motion mobile business view overflows horizontally');
	await explainer.screenshot({
		path: path.join(outputDir, 'landing-mobile-business-reduced-motion.png'),
		style: componentScreenshotStyle,
	});

	await page.locator('[data-explainer-tab="technical"]').click();
	await page.waitForSelector('[data-explainer-panel="technical"]', { state: 'visible' });
	await page.waitForTimeout(120);
	const technical = await inspectExplainer(page);
	if (technical.selectedView !== 'technical') issues.push('Reduced-motion mobile did not switch to Technical view');
	if (!technical.selectedText.includes('Technical view')) issues.push('Reduced-motion technical tab label is missing');
	if (technical.technicalRunning) issues.push('Reduced-motion technical animation should remain static');
	if (technical.scrollWidth > technical.clientWidth + 1) issues.push('Reduced-motion mobile technical view overflows horizontally');
	await explainer.screenshot({
		path: path.join(outputDir, 'landing-mobile-technical-reduced-motion.png'),
		style: componentScreenshotStyle,
	});

	await context.close();
	return { business, technical, issues };
}

async function main() {
	await fs.mkdir(outputDir, { recursive: true });
	const browser = await chromium.launch({ headless: true });
	try {
		const desktop = await captureDesktop(browser);
		const mobile = await captureMobileReduced(browser);
		const issues = [...desktop.issues, ...mobile.issues];
		const report = {
			siteUrl,
			capturedAt: new Date().toISOString(),
			desktop,
			mobile,
			passed: issues.length === 0,
			issues,
		};
		await fs.writeFile(path.join(outputDir, 'explainer-qa.json'), `${JSON.stringify(report, null, 2)}\n`);
		console.log(`Business view: ${desktop.business.selectedText}`);
		console.log(`Technical view: ${desktop.technical.selectedText}`);
		for (const issue of issues) console.error(`- ${issue}`);
		if (issues.length > 0) process.exitCode = 1;
	} finally {
		await browser.close();
	}
}

main().catch((error) => {
	console.error(error);
	process.exitCode = 1;
});
