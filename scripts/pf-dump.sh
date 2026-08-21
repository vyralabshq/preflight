#!/usr/bin/env bash
# Collect a preflight fixture from a validator host. Read-only.
# Writes only into $HOME/pf-dump and $HOME/pf-dump.tgz.
# Redacts SOLANA_METRICS_CONFIG credentials. Never reads keypair contents.

OUT="$HOME/pf-dump"
rm -rf "$OUT"; mkdir -p "$OUT"
run() { l="$1"; shift; { echo "\$ $*"; eval "$@" 2>&1; } > "$OUT/$l.txt"; }

run os        'cat /etc/os-release'
run uname     'uname -a'
run cpuinfo   'cat /proc/cpuinfo'
run meminfo   'cat /proc/meminfo'
run virt      'systemd-detect-virt; cat /sys/class/dmi/id/product_name 2>/dev/null'
run governor  'cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor | sort | uniq -c'
run thp       'cat /sys/kernel/mm/transparent_hugepage/enabled'
run swap      'free -h; swapon --show'
run version   'agave-validator --version; solana --version'
run solcfg    'solana config get'

UNIT=$(systemctl list-units --type=service --all --no-legend 2>/dev/null \
       | awk '{print $1}' | grep -iE 'sol|agave|validator' | head -1)
echo "$UNIT" > "$OUT/unit-name.txt"
run unit      "systemctl cat '$UNIT'"
run unitshow  "systemctl show '$UNIT' -p ExecStart -p User -p Environment -p EnvironmentFile -p LimitNOFILE -p LimitMEMLOCK -p LimitNPROC -p AmbientCapabilities -p CapabilityBoundingSet -p Restart -p RestartSec"

EXEC=$(systemctl show "$UNIT" -p ExecStart --value 2>/dev/null | grep -oE '/[^ ;]+' | head -1)
echo "$EXEC" > "$OUT/exec-target.txt"
[ -f "$EXEC" ] && run wrapper "cat '$EXEC'"

PID=$(pgrep -f 'agave-validator|solana-validator' | head -1)
echo "$PID" > "$OUT/pid.txt"
if [ -n "$PID" ]; then
  run cmdline "tr '\\0' '\\n' < /proc/$PID/cmdline"
  run limits  "cat /proc/$PID/limits"
  run status  "grep -E 'Name|Uid|Gid|Cap' /proc/$PID/status"
  run environ "tr '\\0' '\\n' < /proc/$PID/environ"
fi

run sysctl_rt 'for f in net/core/rmem_max net/core/wmem_max net/core/rmem_default net/core/wmem_default net/core/optmem_max net/core/netdev_max_backlog vm/max_map_count vm/swappiness fs/nr_open fs/file-max; do printf "%-34s %s\n" "$f" "$(cat /proc/sys/$f 2>/dev/null)"; done'
run sysctl_d  'grep -r . /etc/sysctl.d/ /etc/sysctl.conf 2>/dev/null'
run limits_d  'grep -r . /etc/security/limits.conf /etc/security/limits.d/ 2>/dev/null'
run sysdconf  'grep -vE "^\s*#|^\s*$" /etc/systemd/system.conf 2>/dev/null'
run mounts    'cat /proc/mounts'
run lsblk     'lsblk -o NAME,ROTA,TYPE,SIZE,MODEL,MOUNTPOINT'
run df        'df -hT'
run iplink    'ip -br link; echo ---; ip -br addr; echo ---; ip route show default'

IF=$(ip route show default 2>/dev/null | awk '{print $5}' | head -1)
echo "$IF" > "$OUT/iface.txt"
if [ -n "$IF" ]; then
  run nic "cat /sys/class/net/$IF/speed /sys/class/net/$IF/mtu /sys/class/net/$IF/device/vendor /sys/class/net/$IF/device/device 2>/dev/null"
fi
run timesync 'timedatectl; echo ---; chronyc tracking 2>/dev/null || ntpq -p 2>/dev/null'

if [ "$1" = "--with-sudo" ]; then
  {
    echo "=== ethtool -i ==="; sudo ethtool -i "$IF"
    echo "=== ethtool -g ==="; sudo ethtool -g "$IF"
    echo "=== caps ===";       sudo grep -E '^Cap' "/proc/$PID/status"
    echo "=== listening ===";  sudo ss -tulpn
    echo "=== firewall ===";   sudo ufw status verbose 2>/dev/null || sudo nft list ruleset 2>/dev/null || sudo iptables -S
  } > "$OUT/root.txt" 2>&1
fi

sed -i -E 's/(u=)[^,"[:space:]]*/\1REDACTED/g; s/(p=)[^,"[:space:]]*/\1REDACTED/g' "$OUT"/*.txt 2>/dev/null
sed -i -E 's/(password|passwd|secret|token)=[^,"[:space:]]*/\1=REDACTED/gI' "$OUT"/*.txt 2>/dev/null

tar czf "$HOME/pf-dump.tgz" -C "$HOME" pf-dump
echo "done -> $HOME/pf-dump.tgz"
echo "check before sharing:"
grep -riE 'u=|p=|password|token' "$OUT" | head
