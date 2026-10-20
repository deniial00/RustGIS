// Real-browser navigation benchmark, run against a locally served release build:
// playwright-cli open 'http://localhost:8080/?blank&style=https%3A%2F%2Ftiles.openfreemap.org%2Fstyles%2Fliberty#14/40.75/-73.98'
// playwright-cli run-code --filename=apps/rustgis-web/examples/tile_interaction_bench.js
// For a raster run, replace ?style= with ?tiles=<URL-encoded XYZ template>.
// Uses a disposable browser profile. Screenshots go under the ignored target/ directory.
// Measures browser animation-frame intervals, including UI/paint stalls, after loading tiles.
// The 50 ms p95 / 100 ms maximum budget is a manual regression check, not a CI timing test.
async page => {
  const {target, kind} = await page.evaluate(() => {
    const target = new URL(location.href);
    target.searchParams.delete('embed');
    target.searchParams.delete('blank');
    if (!target.searchParams.has('tiles') && !target.searchParams.has('style')) {
      throw new Error('Open a local app URL with ?style= or ?tiles= first');
    }
    // `blank` is a flag; `blank=` would load the World Atlas sample too.
    target.search = '?blank&' + target.searchParams.toString();
    return {target: target.href, kind: target.searchParams.has('tiles') ? 'raster' : 'vector'};
  });
  // A new owned tab avoids the original page's unsaved-changes guard.
  page = await page.context().newPage();
  await page.bringToFront();
  await page.addInitScript(() => {
    // The automated run replaces its own unsaved test projects between modes.
    const listen = window.addEventListener;
    window.addEventListener = function (type, ...args) {
      if (type !== 'beforeunload') return listen.call(this, type, ...args);
    };
  });
  await page.setViewportSize({width: 1480, height: 900});
  const results = [];
  for (const gpu of [false, true]) {
    await page.goto(target);
    await page.evaluate(gpu => localStorage.setItem('rustgis.ui.map.gpu', gpu ? '1' : '0'), gpu);
    await page.reload();
    await page.waitForTimeout(6500);
    await page.evaluate(() => {
      window.__tileFrames = [];
      window.__tileLast = null;
      window.__tileCollect = true;
      function tick(t) {
        if (!window.__tileCollect) return;
        if (window.__tileLast !== null) window.__tileFrames.push(t - window.__tileLast);
        window.__tileLast = t;
        requestAnimationFrame(tick);
      }
      requestAnimationFrame(tick);
    });
    await page.mouse.move(800, 500);
    await page.mouse.down();
    for (let i = 0; i < 60; i++) {
      await page.mouse.move(800 + Math.sin(i / 15) * 250, 500);
      await page.waitForTimeout(16);
    }
    await page.mouse.up();
    for (let i = 0; i < 60; i++) {
      await page.mouse.wheel(0, i < 30 ? -5 : 5);
      await page.waitForTimeout(16);
    }
    const timing = await page.evaluate(() => {
      window.__tileCollect = false;
      const a = window.__tileFrames.sort((a, b) => a - b);
      return {
        frames: a.length,
        mean: a.reduce((a, b) => a + b, 0) / a.length,
        p95: a[Math.floor(a.length * .95)],
        max: a.at(-1),
        over50: a.filter(x => x > 50).length,
      };
    });
    await page.waitForTimeout(1500);
    await page.screenshot({path: `target/tile-browser-bench-${kind}-${gpu ? 'gpu' : 'cpu'}.png`});
    results.push({kind, gpu, ...timing});
  }
  await page.close();
  if (results.some(r => !r.frames || r.p95 > 50 || r.max > 100)) {
    throw new Error(`Navigation frame budget exceeded: ${JSON.stringify(results)}`);
  }
  return results;
}
