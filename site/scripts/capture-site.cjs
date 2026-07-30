const fs = require('node:fs/promises');
const path = require('node:path');
const { chromium } = require('playwright');

const siteUrl = process.env.SITE_URL || 'http://127.0.0.1:4321/openspine/';
const outputDir = path.resolve(process.cwd(), 'visual-artifacts');

const expectedHeading = 'Put a personal AI to work. Keep control of your accounts.';

function localRequestFailed(url) {
	try {
		const target = new URL(url);
		const base = new URL(siteUrl);
		return target.origin === base.origin;
	} catch {
		return true;
	}
}

async function activateViewportReveals(page) {
	const revealItems = page.locator('[data-reveal]');
	const revealCount = await revealItems.count();

	for (let index = 0; index < revealCount; index += 1) {
		await revealItems.nth(index).scrollIntoViewIfNeeded();
		await page.waitForTimeout(180);
	}

	await page.evaluate(() => window.scrollTo(0, 0));
	await page.waitForTimeout(900);
}

async function capture(browser, config) {
	const context = await browser.newContext({
		viewport: config.viewport,
		deviceScaleFactor: 1,
		colorScheme: 'light',
		reducedMotion: config.reducedMotion,
	});
	const page = await context.newPage();
	const consoleErrors = [];
	const pageErrors = [];
	const failedRequests = [];

	page.on('console', (message) => {
		if (message.type() !== 'error') return;
		const text = message.text();
		if (/fonts\.(googleapis|gstatic)\.com/.test(text)) return;
		consoleErrors.push(text);
	});
	page.on('pageerror', (error) => pageErrors.push(error.message));
	page.on('requestfailed', (request) => {
		if (!localRequestFailed(request.url())) return;
		failedRequests.push(`${request.method()} ${request.url()} — ${request.failure()?.errorText || 'failed'}`);
	});

	await page.goto(siteUrl, { waitUntil: 'networkidle', timeout: 60_000 });
	await page.waitForSelector('h1', { state: 'visible', timeout: 15_000 });
	await page.waitForSelector('.authority-trace', { state: 'visible', timeout: 15_000 });
	await page.waitForSelector('.lyra-scenario', { state: 'attached', timeout: 15_000 });
	await page.waitForTimeout(config.settleMs);
	await activateViewportReveals(page);

	const checks = await page.evaluate(async () => {
		const heading = document.querySelector('h1')?.innerText?.replace(/\s+/g, ' ').trim() || '';
		const primaryAction = document.querySelector('.hero-actions .button--primary');
		const quickstartAction = Array.from(document.querySelectorAll('.hero-actions a')).find((link) =>
			link.getAttribute('href')?.endsWith('/quickstart/'),
		);
		const heroAudienceText = document.querySelector('.hero-audience')?.innerText?.replace(/\s+/g, ' ').trim() || '';
		const viewportWidth = document.documentElement.clientWidth;
		const rawHorizontalOverflow = Math.max(0, document.documentElement.scrollWidth - viewportWidth);

		const isClippedByAncestor = (element) => {
			let ancestor = element.parentElement;
			while (ancestor && ancestor !== document.body) {
				const overflowX = window.getComputedStyle(ancestor).overflowX;
				if (overflowX === 'hidden' || overflowX === 'clip') return true;
				ancestor = ancestor.parentElement;
			}
			return false;
		};

		const overflowOffenders = Array.from(document.body.querySelectorAll('*'))
			.map((element) => {
				const rect = element.getBoundingClientRect();
				return { element, rect };
			})
			.filter(({ element, rect }) => {
				if (rect.width === 0 || rect.height === 0) return false;
				if (isClippedByAncestor(element)) return false;
				return rect.left < -1 || rect.right > viewportWidth + 1;
			})
			.slice(0, 20)
			.map(({ element, rect }) => ({
				element: `${element.tagName.toLowerCase()}${element.id ? `#${element.id}` : ''}${Array.from(element.classList)
					.map((name) => `.${name}`)
					.join('')}`,
				left: Math.round(rect.left * 10) / 10,
				right: Math.round(rect.right * 10) / 10,
				width: Math.round(rect.width * 10) / 10,
				parent: element.parentElement
					? `${element.parentElement.tagName.toLowerCase()}${Array.from(element.parentElement.classList)
						.map((name) => `.${name}`)
						.join('')}`
					: null,
			}));

		window.scrollTo(10_000, 0);
		await new Promise((resolve) => window.requestAnimationFrame(() => resolve()));
		const maximumHorizontalScroll = Math.max(0, window.scrollX);
		window.scrollTo(0, 0);

		return {
			title: document.title,
			heading,
			hasAuthorityTrace: Boolean(document.querySelector('.authority-trace')),
			hasLyraScenario: Boolean(document.querySelector('.lyra-scenario')),
			primaryActionText: primaryAction?.innerText?.replace(/\s+/g, ' ').trim() || '',
			primaryActionHref: primaryAction?.getAttribute('href') || '',
			quickstartActionText: quickstartAction?.innerText?.replace(/\s+/g, ' ').trim() || '',
			quickstartActionHref: quickstartAction?.getAttribute('href') || '',
			heroAudienceText,
			documentWidth: document.documentElement.scrollWidth,
			viewportWidth,
			rawHorizontalOverflow,
			maximumHorizontalScroll,
			overflowOffenders,
			unrevealedCount: document.querySelectorAll('[data-reveal]:not(.is-visible)').length,
			traceRunning: document.querySelector('.authority-trace')?.classList.contains('is-running') || false,
		};
	});

	const issues = [];
	if (checks.heading !== expectedHeading) issues.push(`Unexpected H1: ${checks.heading}`);
	if (!checks.hasAuthorityTrace) issues.push('Authority trace is missing');
	if (!checks.hasLyraScenario) issues.push('Lyra scenario is missing');
	if (!checks.primaryActionText.includes('See the limits in action')) issues.push('Primary limits action is missing');
	if (checks.primaryActionHref !== '#mechanism') issues.push(`Unexpected primary action href: ${checks.primaryActionHref}`);
	if (!checks.quickstartActionText.includes('Run the alpha')) issues.push('Hero quickstart action is missing');
	if (!checks.quickstartActionHref.endsWith('/quickstart/')) issues.push(`Unexpected quickstart href: ${checks.quickstartActionHref}`);
	if (!checks.heroAudienceText.includes('Interested in OpenClaw or Hermes?')) issues.push('Personal-agent recognition line is missing');
	if (!checks.heroAudienceText.includes('What works now?')) issues.push('Current proof line is missing');
	if (checks.maximumHorizontalScroll > 1) {
		issues.push(`Page can scroll horizontally by ${checks.maximumHorizontalScroll}px`);
	}
	if (checks.unrevealedCount > 0) issues.push(`${checks.unrevealedCount} reveal elements never became visible`);
	if (config.reducedMotion === 'no-preference' && !checks.traceRunning) issues.push('Authority trace did not enter its running state');
	issues.push(...consoleErrors.map((error) => `Console error: ${error}`));
	issues.push(...pageErrors.map((error) => `Page error: ${error}`));
	issues.push(...failedRequests.map((error) => `Request failure: ${error}`));

	const screenshotPath = path.join(outputDir, `${config.name}.png`);
	await page.screenshot({ path: screenshotPath, fullPage: true });
	await context.close();

	return {
		name: config.name,
		viewport: config.viewport,
		reducedMotion: config.reducedMotion,
		screenshot: path.basename(screenshotPath),
		checks,
		issues,
	};
}

async function main() {
	await fs.rm(outputDir, { recursive: true, force: true });
	await fs.mkdir(outputDir, { recursive: true });

	const browser = await chromium.launch({ headless: true });
	try {
		const results = [];
		results.push(
			await capture(browser, {
				name: 'landing-desktop',
				viewport: { width: 1440, height: 1000 },
				reducedMotion: 'no-preference',
				settleMs: 3_600,
			}),
		);
		results.push(
			await capture(browser, {
				name: 'landing-mobile-reduced-motion',
				viewport: { width: 390, height: 844 },
				reducedMotion: 'reduce',
				settleMs: 250,
			}),
		);

		const report = {
			siteUrl,
			capturedAt: new Date().toISOString(),
			results,
			passed: results.every((result) => result.issues.length === 0),
		};
		await fs.writeFile(path.join(outputDir, 'visual-qa.json'), `${JSON.stringify(report, null, 2)}\n`);

		for (const result of results) {
			console.log(`${result.name}: ${result.issues.length === 0 ? 'passed' : 'failed'}`);
			console.log(`  raw overflow: ${result.checks.rawHorizontalOverflow}px; scrollable: ${result.checks.maximumHorizontalScroll}px`);
			console.log(`  unrevealed elements: ${result.checks.unrevealedCount}`);
			for (const offender of result.checks.overflowOffenders) {
				console.log(`  overflow candidate: ${offender.element} [${offender.left}, ${offender.right}] parent=${offender.parent}`);
			}
			for (const issue of result.issues) console.error(`  - ${issue}`);
		}

		if (!report.passed) process.exitCode = 1;
	} finally {
		await browser.close();
	}
}

main().catch((error) => {
	console.error(error);
	process.exitCode = 1;
});
