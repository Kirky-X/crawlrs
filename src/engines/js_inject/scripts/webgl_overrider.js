// Source: crawlrs original (src/engines/js_inject/scripts/webgl_overrider.js)
// License: Apache-2.0
//
// WebGL / canvas 指纹防护（原创脚本，非 crawl4ai 移植）：
//   - hook WebGLRenderingContext.prototype.getParameter 与 WebGL2 同名方法：
//       UNMASKED_VENDOR_WEBGL  → "Google Inc. (Intel)"
//       UNMASKED_RENDERER_WEBGL → "ANGLE (Intel, Intel(R) Iris(R) Xe Graphics (0x00004626) Direct3D11 vs_5_0 ps_5_0, D3D11)"
//       VENDOR / RENDERER       → 通用值（"WebKit" / "WebKit WebGL"）
//       其余参数透传原始 getParameter（原方法保留并包装，其余行为不变）
//   - canvas 指纹噪声：包装 HTMLCanvasElement.prototype.toDataURL 与 2d 上下文
//       的 getImageData，对像素做极小随机扰动（每通道 ~2% 概率 ±1/255），
//       同一画布两次导出结果不同，破坏 canvas 指纹的跨请求稳定性；
//       扰动幅度在视觉上不可察觉。
//
// 实现说明：
//   - 用 Proxy 包装原生方法：Function.prototype.toString 对 Proxy 返回目标的
//     "[native code]" 源文本，抗 fn.toString() 检测。
//   - toDataURL 在离屏副本上加噪后导出，不污染页面可见画布；
//     tainted canvas（跨域像素）等异常时回退原始调用。
//   - 本脚本不设置 window.__crawlrs_stealth_installed（该标记由
//     navigator_overrider.js 统一管理），避免幂等语义分叉。
(function () {
    'use strict';

    // ---- 1. WebGL getParameter 覆盖 ----

    var UNMASKED_VENDOR_WEBGL = 0x9245;
    var UNMASKED_RENDERER_WEBGL = 0x9246;
    var VENDOR = 0x1f00;
    var RENDERER = 0x1f01;

    var FAKE_UNMASKED_VENDOR = 'Google Inc. (Intel)';
    var FAKE_UNMASKED_RENDERER = 'ANGLE (Intel, Intel(R) Iris(R) Xe Graphics (0x00004626) Direct3D11 vs_5_0 ps_5_0, D3D11)';
    var FAKE_VENDOR = 'WebKit';
    var FAKE_RENDERER = 'WebKit WebGL';

    // 用 Proxy 包装原生方法：hook 收到 (thisArg, args, orig)，
    // orig(self) 可在任意接收者上调用原始方法（toDataURL 需要换到副本画布上）。
    function wrapFunction(target, hook) {
        return new Proxy(target, {
            apply: function (_target, thisArg, args) {
                return hook(thisArg, args, function (self) {
                    return Reflect.apply(target, self || thisArg, args);
                });
            },
        });
    }

    function hookGetParameter(proto) {
        if (!proto || typeof proto.getParameter !== 'function') {
            return;
        }
        proto.getParameter = wrapFunction(proto.getParameter, function (_self, args, orig) {
            switch (args[0]) {
                case UNMASKED_VENDOR_WEBGL:
                    return FAKE_UNMASKED_VENDOR;
                case UNMASKED_RENDERER_WEBGL:
                    return FAKE_UNMASKED_RENDERER;
                case VENDOR:
                    return FAKE_VENDOR;
                case RENDERER:
                    return FAKE_RENDERER;
                default:
                    return orig();
            }
        });
    }

    if (typeof WebGLRenderingContext !== 'undefined') {
        hookGetParameter(WebGLRenderingContext.prototype);
    }
    if (typeof WebGL2RenderingContext !== 'undefined') {
        hookGetParameter(WebGL2RenderingContext.prototype);
    }

    // ---- 2. canvas 像素噪声 ----

    var NOISE_PROBABILITY = 0.02; // 每个通道 ~2% 概率被扰动

    // 就地扰动像素数组：~2% 概率对单通道 ±1，clamp 到 [0, 255]
    function addPixelNoise(data) {
        for (var i = 0; i < data.length; i++) {
            if (Math.random() < NOISE_PROBABILITY) {
                var value = data[i] + (Math.random() < 0.5 ? -1 : 1);
                data[i] = value < 0 ? 0 : value > 255 ? 255 : value;
            }
        }
        return data;
    }

    var canvasProto = typeof HTMLCanvasElement !== 'undefined' ? HTMLCanvasElement.prototype : null;
    var ctx2dProto = typeof CanvasRenderingContext2D !== 'undefined' ? CanvasRenderingContext2D.prototype : null;

    if (!canvasProto || !ctx2dProto || typeof ctx2dProto.getImageData !== 'function') {
        return;
    }

    // 先保存原始 getImageData：toDataURL 内部复用原始方法，避免经过已包装
    // 版本导致副本像素与 getImageData 返回值双重扰动、幅度不可控
    var rawGetImageData = ctx2dProto.getImageData;

    // getImageData：读取结果加噪 → 指纹脚本两次读取得到不同数据
    ctx2dProto.getImageData = wrapFunction(rawGetImageData, function (_self, args, orig) {
        var imageData = orig();
        try {
            addPixelNoise(imageData.data);
        } catch (e) {
            // 像素扰动失败时返回原始数据，绝不让 stealth 脚本中断页面逻辑
        }
        return imageData;
    });

    // toDataURL：在离屏副本上加噪后导出，不污染可见画布本身
    canvasProto.toDataURL = wrapFunction(canvasProto.toDataURL, function (canvas, args, orig) {
        try {
            var ctx = canvas.getContext('2d');
            if (!ctx || canvas.width === 0 || canvas.height === 0) {
                return orig();
            }
            var imageData = rawGetImageData.call(ctx, 0, 0, canvas.width, canvas.height);
            addPixelNoise(imageData.data);
            var copy = document.createElement('canvas');
            copy.width = canvas.width;
            copy.height = canvas.height;
            copy.getContext('2d').putImageData(imageData, 0, 0);
            return orig(copy);
        } catch (e) {
            // tainted canvas（跨域像素读取抛 SecurityError）等场景：回退原始行为
            return orig();
        }
    });
})();
