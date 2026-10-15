'use strict';
const assert = require('node:assert/strict');

// Use Select's supported keyboard protocol and observable option identity.
// Virtual-list rendering and the managed UI's private CSS prefix are irrelevant.
async function chooseApplication({ page, form, label, applicationId, deadlineMs }) {
  const combo = form.getByLabel(label, {exact:true});
  assert.equal(await combo.getAttribute('role'), 'combobox', 'label associates with its Select');
  await combo.click();
  const comboId = await combo.getAttribute('id');
  assert.ok(comboId, 'Select has an accessible field identity');
  const deadline = Date.now() + deadlineMs, seen = new Set();
  for (;;) {
    const remaining = deadline - Date.now();
    assert.ok(remaining > 0, 'application keyboard selection deadline');
    // Locator queries pierce the managed surface's open ShadowRoot.
    // document.getElementById cannot see these fields or their popup options.
    await form.locator('[role="combobox"][aria-activedescendant]').waitFor({timeout:remaining});
    const activeId = await combo.getAttribute('aria-activedescendant');
    const value = (await page.locator(`[id=${JSON.stringify(activeId)}]`).textContent()).trim();
    assert.ok(value, 'active Select option exposes its real application ID');
    if (value === applicationId) {
      await combo.press('Enter');
      await form.locator('[role="combobox"][aria-expanded="false"]').waitFor();
      return {application_id:applicationId, visited_options:seen.size + 1};
    }
    assert.ok(!seen.has(value), 'application absent from complete keyboard option cycle');
    seen.add(value);
    await combo.press('ArrowDown');
    await form.locator(`[role="combobox"][aria-activedescendant]:not([aria-activedescendant=${JSON.stringify(activeId)}])`)
      .waitFor({timeout:Math.max(1, deadline - Date.now())});
  }
}
module.exports = { chooseApplication };
