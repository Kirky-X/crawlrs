# 分支管理策略

本文档描述 crawlrs 项目的 Git 分支管理策略。

## 分支命名规范

### 主分支

| 分支名称 | 用途 | 保护规则 |
|---------|------|----------|
| `main` | 生产环境代码 | 必须通过 PR 合并，禁止直接推送 |
| `develop` | 开发主分支 | 必须通过 PR 合并，禁止直接推送 |

### 特性分支

```
格式: feature/{issue-id}-{short-description}
示例: feature/123-add-rate-limiting
```

### 发布分支

```
格式: release/v{version}
示例: release/v0.2.0
```

### 热修复分支

```
格式: hotfix/{issue-id}-{short-description}
示例: hotfix/456-fix-security-vulnerability
```

## 工作流程

### 1. 功能开发

```bash
# 1. 从 develop 创建特性分支
git checkout -b feature/123-add-api-cache develop

# 2. 开发并提交
git add .
git commit -m "feat: add API key caching mechanism"

# 3. 推送并创建 PR
git push -u origin feature/123-add-api-cache
```

### 2. 发布流程

```bash
# 1. 从 develop 创建发布分支
git checkout -b release/v0.2.0 develop

# 2. 更新版本号
# ... 版本更新操作 ...

# 3. 完成发布
git checkout main
git merge release/v0.2.0 --no-ff
git tag -a v0.2.0 -m "Release v0.2.0"

# 4. 同步到 develop
git checkout develop
git merge release/v0.2.0 --no-ff
```

### 3. 热修复

```bash
# 1. 从 main 创建热修复分支
git checkout -b hotfix/456-fix-security-vulnerability main

# 2. 修复并提交
git commit -m "fix: resolve XSS vulnerability in task metadata"

# 3. 合并到 main 和 develop
git checkout main
git merge hotfix/456-fix-security-vulnerability --no-ff
git tag -a v0.2.1 -m "Hotfix v0.2.1"

git checkout develop
git merge hotfix/456-fix-security-vulnerability --no-ff
```

## 提交规范

### 提交消息格式

```
<type>(<scope>): <subject>

<body>

<footer>
```

### 类型（Type）

| 类型 | 描述 | 示例 |
|------|------|------|
| `feat` | 新功能 | `feat(auth): add API key caching` |
| `fix` | Bug 修复 | `fix(middleware): resolve XSS vulnerability` |
| `docs` | 文档更新 | `docs: update README` |
| `style` | 代码格式 | `style: run cargo fmt` |
| `refactor` | 重构 | `refactor(handler): split query_tasks function` |
| `perf` | 性能优化 | `perf(ssrf): optimize IPv6 matching` |
| `test` | 测试 | `test: add integration tests` |
| `chore` | 构建/工具 | `chore: update dependencies` |

### 范围（Scope）

常用 scope：
- `auth` - 认证相关
- `middleware` - 中间件
- `handler` - 处理器
- `service` - 服务层
- `repository` - 数据访问层
- `config` - 配置
- `docs` - 文档
- `ci` - CI/CD

### 示例

```
feat(auth): add API key expiration check

- Implement 90-day expiration policy
- Add cache for validated API keys
- Improve error message handling

Closes #123
```

## 合并策略

### Pull Request 要求

- [ ] 所有 CI 检查通过
- [ ] 代码审查通过（至少 1 人）
- [ ] 测试覆盖新增代码
- [ ] 文档已更新
- [ ] 无 lint 错误

### 合并方式

- **Squash and merge**: 用于小型 PR
- **Rebase and merge**: 用于线性历史
- **Create a merge commit**: 用于重要特性

## 版本号规范

采用语义化版本（Semantic Versioning）：

```
MAJOR.MINOR.PATCH

- MAJOR: 不兼容的 API 变更
- MINOR: 新功能（向后兼容）
- PATCH: Bug 修复（向后兼容）
```

## 标签（Tags）

```bash
# 版本标签
git tag -a v0.1.0 -m "Version 0.1.0"

# 预发布标签
git tag -a v0.2.0-alpha -m "Alpha release"

# 提交标签
git tag -a commit-hash -m "Message"
```

## 快速参考

```bash
# 创建特性分支
git checkout -b feature/123-description develop

# 完成特性后
git checkout develop
git merge feature/123-description --no-ff
git branch -d feature/123-description

# 创建发布分支
git checkout -b release/v0.2.0 develop

# 完成发布
git checkout main
git merge release/v0.2.0 --no-ff
git tag -a v0.2.0 -m "Release v0.2.0"

# 创建热修复
git checkout -b hotfix/456-fix main
# ... 修复 ...
git checkout main
git merge hotfix/456-fix --no-ff
git tag -a v0.2.1 -m "Hotfix v0.2.1"
```
