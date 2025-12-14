#!/bin/bash

# 清理未使用的 Docker 镜像脚本
echo "开始清理未使用的 Docker 镜像..."

# 当前项目使用的镜像列表
USED_IMAGES=("redis:7-alpine" "postgres:15-alpine")

# 获取所有镜像列表
echo "获取所有 Docker 镜像..."
ALL_IMAGES=$(docker image ls --format "{{.Repository}}:{{.Tag}}" | grep -v "^<none>")

# 创建要删除的镜像列表
IMAGES_TO_REMOVE=()

echo "识别未使用的镜像..."
while IFS= read -r image; do
    if [[ ! " ${USED_IMAGES[@]} " =~ " ${image} " ]]; then
        IMAGES_TO_REMOVE+=("$image")
    fi
done <<< "$ALL_IMAGES"

# 显示将要删除的镜像
echo "以下镜像将被删除:"
for image in "${IMAGES_TO_REMOVE[@]}"; do
    echo "  - $image"
done

# 确认删除
echo ""
read -p "确认删除以上镜像吗? (y/N): " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "开始删除镜像..."
    for image in "${IMAGES_TO_REMOVE[@]}"; do
        echo "正在删除 $image..."
        docker rmi "$image" 2>/dev/null || echo "跳过 $image (可能正在使用或其他原因)"
    done
    echo "镜像清理完成!"
else
    echo "操作已取消."
fi

# 清理悬空镜像 (dangling images)
echo ""
echo "清理悬空镜像..."
docker image prune -f

echo "所有清理操作已完成!"
