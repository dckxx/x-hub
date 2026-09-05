#!/usr/bin/env bash
# x-hub 自建分发服务器一键初始化（Ubuntu/Debian，幂等：重复执行安全）
# 配套文档：docs/self-hosted-distribution.md（本仓库）
#
# 用法（本地打包上传后，在服务器上以 root 执行）：
#   sudo bash init-server.sh \
#     --port 8080 \
#     --pubkey "ssh-ed25519 AAAA... xhub-deploy-local" \
#     --pubkey "ssh-ed25519 BBBB... xhub-deploy-ci"
# 可选：--dist-root /srv/x-hub-dist  --user deploy
#
# 本脚本做：装 nginx → 建分发目录 → 建 deploy 账号 → 装公钥 → 写站点配置 → reload → 放行防火墙
set -euo pipefail

PORT=8080
DIST_ROOT=/srv/x-hub-dist
DEPLOY_USER=deploy
PUBKEYS=()
CONF_SRC="$(cd "$(dirname "$0")" && pwd)/x-hub-dist.conf"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --port)      PORT="$2"; shift 2 ;;
    --dist-root) DIST_ROOT="$2"; shift 2 ;;
    --user)      DEPLOY_USER="$2"; shift 2 ;;
    --pubkey)    PUBKEYS+=("$2"); shift 2 ;;
    *) echo "未知参数: $1" >&2; exit 1 ;;
  esac
done

echo "==> [1/7] 安装 nginx"
if ! command -v nginx >/dev/null 2>&1; then
  apt-get update -qq
  DEBIAN_FRONTEND=noninteractive apt-get install -y -qq nginx
fi

echo "==> [2/7] 创建分发目录 $DIST_ROOT/{extensions,releases}"
mkdir -p "$DIST_ROOT/extensions" "$DIST_ROOT/releases"

echo "==> [3/7] 创建部署账号 $DEPLOY_USER"
if ! id -u "$DEPLOY_USER" >/dev/null 2>&1; then
  useradd -m -s /bin/bash "$DEPLOY_USER"
fi

echo "==> [4/7] 目录属主与权限（nginx 以 other 可读访问）"
chown -R "$DEPLOY_USER:$DEPLOY_USER" "$DIST_ROOT"
chmod 755 "$DIST_ROOT"

echo "==> [5/7] 写入 authorized_keys（去重追加，${#PUBKEYS[@]} 把公钥）"
install -d -m 700 -o "$DEPLOY_USER" -g "$DEPLOY_USER" "/home/$DEPLOY_USER/.ssh"
AUTHKEYS="/home/$DEPLOY_USER/.ssh/authorized_keys"
touch "$AUTHKEYS"
for key in "${PUBKEYS[@]:-}"; do
  [[ -z "$key" ]] && continue
  if ! grep -qxF "$key" "$AUTHKEYS"; then
    echo "$key" >> "$AUTHKEYS"
  fi
done
chown "$DEPLOY_USER:$DEPLOY_USER" "$AUTHKEYS"
chmod 600 "$AUTHKEYS"

echo "==> [6/7] 写入 nginx 站点配置（listen $PORT → $DIST_ROOT）"
sed -e "s|__PORT__|$PORT|g" -e "s|__ROOT__|$DIST_ROOT|g" "$CONF_SRC" \
  > /etc/nginx/conf.d/x-hub-dist.conf
nginx -t
systemctl reload nginx

echo "==> [7/7] 防火墙放行 $PORT/tcp"
if command -v ufw >/dev/null 2>&1 && ufw status | grep -q "Status: active"; then
  ufw allow "$PORT/tcp"
else
  echo "    ufw 未启用，跳过（请确认云安全组已放行 TCP $PORT 与 22）"
fi

echo ""
echo "初始化完成 ✔  接下来："
echo "  1) 云厂商安全组放行 TCP $PORT（若尚未放行）"
echo "  2) 在【本地开发机】从 R2 拉平存量（需 R2_* 与 XHUB_DEPLOY_* 环境变量，见文档 §6 阶段 0）："
echo "     rclone sync r2:x-hub-dist/extensions xhubsftp:$DIST_ROOT/extensions"
echo "     rclone sync r2:x-hub-dist/releases  xhubsftp:$DIST_ROOT/releases"
echo "  3) 本地验证："
for p in extensions/registry.json releases/update.json; do
  echo "     curl -sI http://<本机公网IP>:$PORT/$p | head -1"
done
echo "     curl -sI -H 'Range: bytes=0-99' http://<本机公网IP>:$PORT/releases/update.json | head -1   # 应 206"
