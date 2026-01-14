#!/bin/bash
# Copyright (c) 2025 Kirky.X
#
# Licensed under the Apache License, Version 2.0
# See LICENSE file in the project root for full license information.

# Entrypoint script for crawlrs container

echo "[entrypoint] Setting up DNS configuration..."

# Backup and configure DNS
[ -f /etc/resolv.conf ] && cp /etc/resolv.conf /etc/resolv.conf.backup
cat > /etc/resolv.conf << 'EOF'
nameserver 8.8.8.8
nameserver 114.114.114.114
options timeout:2 attempts:3
EOF

echo "[entrypoint] DNS configuration updated:"
cat /etc/resolv.conf

echo "[entrypoint] Starting crawlrs..."
exec "$@"