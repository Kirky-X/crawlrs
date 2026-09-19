# 运维日志消息（非 worker）— zh-CN
# 覆盖文本处理流水线、启动期 CORS 与引擎运行时日志（29 keys）

# 文本处理流水线（15 keys）
text-short-input = 处理短文本，长度: { $length } 字节
text-long-input = 处理长文本，长度: { $length } 字节
text-cached-detection = 使用缓存的编码检测结果: { $detection }
text-unicode-escapes-detected = 检测到Unicode转义序列，执行规范化转换
text-unicode-escapes-parsed = Unicode转义序列解析完成: { $result }
text-detection-started = 开始编码检测
text-encoding-detected = 检测到编码: { $encoding }，置信度: { $confidence }
text-detection-low-confidence = 编码检测置信度过低: { $confidence }，使用UTF-8尝试解析
text-html-structure-detected = 检测到HTML结构: { $is_html }，声明编码: { $encoding }
text-encoding-succeeded = 文本编码处理成功
text-encoding-failed = 文本编码处理失败: { $error }
text-web-content-started = 开始处理网页内容，大小: { $size } 字节
text-crawl-started = 开始处理抓取的内容: URL={ $url }，大小={ $size } 字节
text-crawl-completed = 内容处理完成: URL={ $url }，耗时={ $elapsed }，提取文本长度={ $length }
text-config-updated = 更新爬虫文本处理器配置: { $config }

# 爬虫文本集成（7 keys）
text-integration-enabled = 启用爬虫文本处理功能
text-integration-disabled = 禁用爬虫文本处理功能
text-integration-disabled-direct = 文本处理功能已禁用，直接返回原始内容
text-integration-succeeded = 文本处理成功: URL={ $url }，提取文本长度={ $length }，语言={ $language }
text-integration-failed = 文本处理失败: URL={ $url }，错误={ $error }
text-integration-disabled-batch = 文本处理功能已禁用，批量返回原始内容
text-integration-item-failed = 批量处理中的单个项目失败: URL={ $url }，错误={ $error }

# 启动期（2 keys）
boot-cors-wildcard = CORS 使用通配符 '*'，建议在生产环境中配置具体的来源
boot-cors-invalid-fallback = CORS 配置无效，允许所有来源作为回退

# 浏览器下载器日志（4 keys）
browser-check-started = 开始检查浏览器...
browser-fetcher-download-failed = fetcher 下载失败: { $error }
browser-cleaned-up = 已清理下载的浏览器
browser-system-found = 找到系统浏览器: { $path }

# 引擎路由（1 key）
engine-registered = 引擎已注册: { $name }
