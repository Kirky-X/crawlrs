// Source: crawl4ai js_snippet/remove_consent_popups.js
// https://github.com/unclecode/crawl4ai/blob/main/crawl4ai/js_snippet/remove_consent_popups.js
// License: Apache-2.0
//
// 移除常见 GDPR / CCPA / cookie consent 弹窗元素：
//   - OneTrust (#onetrust-banner-sdk, #onetrust-consent-sdk, #onetrust-pc-sdk)
//   - Cookiebot (#CybotCookiebotDialog, #CybotCookiebotDialogBody)
//   - Quantcast Choice (.qc-cmp2-container, #qc-cmp2-container)
//   - TrustArc (#truste-consent-track, #consent_blackbar)
//   - Sourcepoint (.sp_message_container, #sp_message_container_*)
//   - Didomi (#didomi-host, .didomi-popup-container)
//   - iubenda (#iubenda-cs-banner, .iubenda-cs-container)
//   - Generic (.cc-banner, .cc-window, .cookie-banner, .consent-banner,
//             .cookies-consent, .gdpr-banner, .privacy-banner)
//
// 同步尝试点击"接受/拒绝全部"按钮以触发站点记录偏好，再移除弹窗容器；
// 失败则直接 remove() 容器。所有操作 try/catch 防止中断脚本。
(function () {
    'use strict';

    var CONSENT_SELECTORS = [
        // OneTrust
        '#onetrust-banner-sdk',
        '#onetrust-consent-sdk',
        '#onetrust-pc-sdk',
        '#onetrust-accept-all-handler',
        // Cookiebot
        '#CybotCookiebotDialog',
        '#CybotCookiebotDialogBody',
        // Quantcast Choice
        '.qc-cmp2-container',
        '#qc-cmp2-container',
        '.qc-cmp2-summary-buttons',
        // TrustArc
        '#truste-consent-track',
        '#consent_blackbar',
        // Sourcepoint
        '.sp_message_container',
        '[id^="sp_message_container_"]',
        // Didomi
        '#didomi-host',
        '.didomi-popup-container',
        // iubenda
        '#iubenda-cs-banner',
        '.iubenda-cs-container',
        // Generic
        '.cc-banner',
        '.cc-window',
        '.cc-overlay',
        '.cookie-banner',
        '.cookie-notice',
        '.cookie-consent',
        '.consent-banner',
        '.consent-popup',
        '.cookies-consent',
        '.gdpr-banner',
        '.gdpr-popup',
        '.privacy-banner',
        '.privacy-notice',
        '[role="dialog"][aria-label*="cookie" i]',
        '[role="dialog"][aria-label*="consent" i]',
        '[role="dialog"][aria-label*="privacy" i]',
    ];

    var ACCEPT_BUTTON_SELECTORS = [
        '#onetrust-accept-all-handler',
        '.qc-cmp2-summary-agree',
        '#CybotCookiebotDialogBodyLevelButtonLevelOptinAllowAll',
        '#truste-consent-button',
        '.sp_choice_type_11', // Sourcepoint "Accept"
        '#didomi-notice-agree-button',
        '.iubenda-cs-accept-btn',
        'button[class*="accept" i]',
        'button[id*="accept" i]',
    ];

    function tryClickAccept() {
        for (var i = 0; i < ACCEPT_BUTTON_SELECTORS.length; i++) {
            var btn = document.querySelector(ACCEPT_BUTTON_SELECTORS[i]);
            if (btn) {
                try {
                    btn.click();
                } catch (e) {}
                return true;
            }
        }
        return false;
    }

    function removeBySelector(selector) {
        var removed = 0;
        var nodes = document.querySelectorAll(selector);
        for (var i = 0; i < nodes.length; i++) {
            var node = nodes[i];
            try {
                // 移除 backdrop + 滚动锁
                if (node.parentNode) {
                    node.parentNode.removeChild(node);
                    removed++;
                }
            } catch (e) {}
        }
        return removed;
    }

    // 先尝试点击"接受全部"以触发站点记录偏好
    tryClickAccept();

    var totalRemoved = 0;
    for (var i = 0; i < CONSENT_SELECTORS.length; i++) {
        totalRemoved += removeBySelector(CONSENT_SELECTORS[i]);
    }

    // 修复 consent 弹窗留下的 body 滚动锁
    try {
        document.body.style.overflow = '';
        document.documentElement.style.overflow = '';
        document.body.style.position = '';
        document.body.style.height = '';
    } catch (e) {}

    try {
        window.__crawlrs_consent_removed_count__ = totalRemoved;
    } catch (e) {}
})();
