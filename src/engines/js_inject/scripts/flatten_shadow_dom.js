// Source: crawl4ai js_snippet/flatten_shadow_dom.js
// https://github.com/unclecode/crawl4ai/blob/main/crawl4ai/js_snippet/flatten_shadow_dom.js
// License: Apache-2.0
//
// 递归遍历文档中所有元素的 shadow DOM，将 shadow root 中的子节点
// "提升"到主文档树（作为该元素的直接子节点），便于后续抓取与选择器命中。
//
// 处理流程：
//   1. 收集文档中所有 element（用 querySelectorAll('*')，包含 open/closed shadow）
//   2. 对每个 element 调用 element.shadowRoot（仅 open 可见；closed 不可见跳过）
//   3. 将 shadowRoot 的所有子节点 appendChild 到宿主元素
//      —— appendChild 会从原父（shadow root）移除，所以无需手动清理
//   4. 因为提升后子树可能引入新的 shadow host，使用广度优先 + 上限循环
//      （最多 10 轮，防止恶意页面构造循环引用导致死循环）
//
// 注意：closed shadow root 无法通过 element.shadowRoot 访问，因此只能展平 open shadow。
// 这是 W3C Shadow DOM 规范的固有约束，并非本脚本的缺陷。
(function () {
    'use strict';

    var MAX_PASSES = 10;
    var pass = 0;
    var flattened = 0;

    function flattenOnce() {
        var hosts = document.querySelectorAll('*');
        var changed = false;
        for (var i = 0; i < hosts.length; i++) {
            var el = hosts[i];
            var root = el.shadowRoot;
            if (!root) continue;
            // 取出所有子节点（动态收集，避免 live NodeList 边遍历边修改）
            var children = [];
            for (var j = 0; j < root.childNodes.length; j++) {
                children.push(root.childNodes[j]);
            }
            for (var k = 0; k < children.length; k++) {
                try {
                    el.appendChild(children[k]); // 自动从 shadowRoot 移除
                    flattened++;
                    changed = true;
                } catch (e) {
                    // 某些节点（如 <style> 跨边界）可能抛 NotFoundError，跳过
                }
            }
        }
        return changed;
    }

    while (pass < MAX_PASSES && flattenOnce()) {
        pass++;
    }

    // 在 window 上暴露本次展平的节点计数，便于调试
    try {
        window.__crawlrs_shadow_flattened_count__ = flattened;
    } catch (e) {}
})();
