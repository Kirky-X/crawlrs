// Source: crawl4ai js_snippet/remove_overlay_elements.js
// https://github.com/unclecode/crawl4ai/blob/main/crawl4ai/js_snippet/remove_overlay_elements.js
// License: Apache-2.0
//
// 移除遮罩类元素：modal / dialog / overlay / backdrop / popup。
// 这些元素在抓取场景下通常不是目标内容，反而会遮挡主内容、阻断交互。
//
// 选择器覆盖：
//   - role="dialog" / role="alertdialog"
//   - .modal / .dialog / .overlay / .backdrop / .popup / .mask
//   - 常见 UI 库前缀：.ant-modal, .el-dialog, .MuiDialog-root, .ui-dialog, .fancybox-overlay
//   - data-* 属性标记：[data-modal], [data-dialog], [data-overlay]
//   - 高 z-index 的全屏 fixed 元素（启发式：z-index >= 9999 且覆盖 80% 视口）
//
// 同时清理 body 滚动锁（overflow:hidden / position:fixed）。
(function () {
    'use strict';

    var OVERLAY_SELECTORS = [
        // ARIA roles
        '[role="dialog"]',
        '[role="alertdialog"]',
        // 通用类名
        '.modal',
        '.modal-backdrop',
        '.modal-dialog',
        '.dialog',
        '.overlay',
        '.backdrop',
        '.popup',
        '.mask',
        '.mask-layer',
        '.cover',
        '.lightbox',
        '.modal-open',
        // UI 库前缀
        '.ant-modal',
        '.ant-modal-mask',
        '.el-dialog',
        '.el-dialog__wrapper',
        '.el-overlay',
        '.MuiDialog-root',
        '.MuiModal-root',
        '.ui-dialog',
        '.ui-widget-overlay',
        '.fancybox-overlay',
        '.modal-mask',
        // data-* 属性
        '[data-modal]',
        '[data-dialog]',
        '[data-overlay]',
        '[data-popup]',
    ];

    var totalRemoved = 0;

    function removeBySelector(selector) {
        var nodes = document.querySelectorAll(selector);
        var removed = 0;
        for (var i = 0; i < nodes.length; i++) {
            var node = nodes[i];
            try {
                // 仅移除"遮罩层"本体：保留可能被嵌套的真实内容
                // 启发式：class 同时包含 modal/dialog/overlay/backdrop 等关键词
                if (node.parentNode) {
                    node.parentNode.removeChild(node);
                    removed++;
                }
            } catch (e) {}
        }
        return removed;
    }

    for (var i = 0; i < OVERLAY_SELECTORS.length; i++) {
        totalRemoved += removeBySelector(OVERLAY_SELECTORS[i]);
    }

    // 启发式：移除高 z-index + 全屏 fixed 遮罩（如未命中上面选择器的自定义实现）
    try {
        var all = document.querySelectorAll('body *');
        for (var j = 0; j < all.length; j++) {
            var el = all[j];
            var cs = getComputedStyle(el);
            if (cs.position !== 'fixed' && cs.position !== 'absolute') continue;
            var z = parseInt(cs.zIndex, 10);
            if (isNaN(z) || z < 9999) continue;
            var rect = el.getBoundingClientRect();
            var vw = window.innerWidth || document.documentElement.clientWidth;
            var vh = window.innerHeight || document.documentElement.clientHeight;
            // 覆盖 >= 80% 视口
            if ((rect.width / vw) >= 0.8 && (rect.height / vh) >= 0.8) {
                try {
                    if (el.parentNode) {
                        el.parentNode.removeChild(el);
                        totalRemoved++;
                    }
                } catch (e) {}
            }
        }
    } catch (e) {}

    // 清理 body 滚动锁
    try {
        document.body.style.overflow = '';
        document.documentElement.style.overflow = '';
        document.body.style.position = '';
        document.body.style.height = '';
        document.body.style.top = '';
        document.body.style.left = '';
        document.body.style.right = '';
    } catch (e) {}

    try {
        window.__crawlrs_overlay_removed_count__ = totalRemoved;
    } catch (e) {}
})();
