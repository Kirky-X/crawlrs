// Source: crawl4ai js_snippet/navigator_overrider.js
// https://github.com/unclecode/crawl4ai/blob/main/crawl4ai/js_snippet/navigator_overrider.js
// License: Apache-2.0
//
// 覆盖 navigator 上的反爬指纹属性：
//   - navigator.webdriver → false（隐藏 Selenium/Puppeteer/Playwright 痕迹）
//   - navigator.languages → ['en-US', 'en']（模拟真实浏览器语言列表）
//   - navigator.plugins → 三个真实插件条目（Chrome PDF Plugin / Chrome PDF Viewer / Native Client）
//   - navigator.mimeTypes → 对应 MIME 类型列表
//   - navigator.platform → 与浏览器 UA 平台对齐的值（取 UA platform）
//   - navigator.hardwareConcurrency → 4（避免暴露真实 CPU 核心数）
//   - navigator.deviceMemory → 8（避免暴露真实内存）
//   - window.chrome → 模拟 Chrome runtime 对象
//   - Notification.permission → 'default'
//   - Permissions.query → 对 'notifications' 返回 'denied' 而非 'prompt'（headless 默认 prompt 暴露）
//
// 通过 Object.defineProperty 在 navigator 原型上挂载 getter，确保多次读取返回一致值；
// 用 try/catch 包裹防止某些属性在特殊浏览器下不可配置时抛错中断脚本。
(function () {
    'use strict';

    function defineGetter(target, prop, value) {
        try {
            Object.defineProperty(target, prop, {
                get: function () { return value; },
                configurable: true,
            });
        } catch (e) {
            // 静默忽略：某些浏览器对该属性锁死，跳过不影响其他覆盖
        }
    }

    // 1. navigator.webdriver = false
    defineGetter(navigator, 'webdriver', false);

    // 2. navigator.languages：真实浏览器一般返回 ['en-US', 'en']
    defineGetter(navigator, 'languages', ['en-US', 'en']);

    // 3. navigator.platform：从 userAgent 推断，与 UA 一致
    var platform = 'Win32';
    var ua = (navigator.userAgent || '').toLowerCase();
    if (ua.indexOf('mac os x') >= 0 || ua.indexOf('macintosh') >= 0) {
        platform = 'MacIntel';
    } else if (ua.indexOf('linux') >= 0) {
        platform = 'Linux x86_64';
    } else if (ua.indexOf('iphone') >= 0 || ua.indexOf('ipad') >= 0) {
        platform = 'iPhone';
    } else if (ua.indexOf('android') >= 0) {
        platform = 'Linux armv8l';
    }
    defineGetter(navigator, 'platform', platform);

    // 4. navigator.plugins：构造三个真实 Chrome 插件
    function fakePlugin(name, filename, description) {
        var p = Object.create(Plugin.prototype);
        Object.defineProperty(p, 'name', { value: name });
        Object.defineProperty(p, 'filename', { value: filename });
        Object.defineProperty(p, 'description', { value: description });
        Object.defineProperty(p, 'length', { value: 1 });
        return p;
    }
    var plugins = [
        fakePlugin('Chrome PDF Plugin', 'internal-pdf-viewer', 'Portable Document Format'),
        fakePlugin('Chrome PDF Viewer', 'internal-pdf-viewer', ''),
        fakePlugin('Native Client', 'internal-nacl-plugin', ''),
    ];
    try {
        Object.defineProperty(navigator, 'plugins', {
            get: function () {
                var arr = [plugins[0], plugins[1], plugins[2]];
                arr.item = function (i) { return arr[i] || null; };
                arr.namedItem = function (n) {
                    for (var i = 0; i < arr.length; i++) {
                        if (arr[i].name === n) return arr[i];
                    }
                    return null;
                };
                arr.refresh = function () {};
                return arr;
            },
            configurable: true,
        });
    } catch (e) {}

    // 5. navigator.mimeTypes：对应 PDF 的两个 MIME
    try {
        Object.defineProperty(navigator, 'mimeTypes', {
            get: function () {
                var arr = [
                    { type: 'application/pdf', suffixes: 'pdf', description: 'Portable Document Format' },
                    { type: 'text/pdf', suffixes: 'pdf', description: '' },
                ];
                arr.item = function (i) { return arr[i] || null; };
                arr.namedItem = function (n) {
                    for (var i = 0; i < arr.length; i++) {
                        if (arr[i].type === n) return arr[i];
                    }
                    return null;
                };
                return arr;
            },
            configurable: true,
        });
    } catch (e) {}

    // 6. navigator.hardwareConcurrency / deviceMemory：避免暴露真实硬件
    defineGetter(navigator, 'hardwareConcurrency', 4);
    try {
        defineGetter(navigator, 'deviceMemory', 8);
    } catch (e) {}

    // 7. window.chrome：模拟 Chrome runtime 对象（headless Chrome 缺失）
    if (typeof window.chrome === 'undefined') {
        window.chrome = {
            runtime: {},
            loadTimes: function () { return {}; },
            csi: function () { return {}; },
            app: {},
        };
    }

    // 8. Notification.permission：headless 默认 'denied'，正常浏览器为 'default'
    if (typeof Notification !== 'undefined') {
        try {
            Object.defineProperty(Notification, 'permission', {
                get: function () { return 'default'; },
                configurable: true,
            });
        } catch (e) {}
    }

    // 9. Permissions.query：对 'notifications' 修正返回 'prompt'（而非 headless 的 'denied'）
    if (navigator.permissions && navigator.permissions.query) {
        var origQuery = navigator.permissions.query.bind(navigator.permissions);
        navigator.permissions.query = function (parameters) {
            if (parameters && parameters.name === 'notifications') {
                return Promise.resolve({ state: 'prompt', onchange: null });
            }
            return origQuery(parameters);
        };
    }
})();
