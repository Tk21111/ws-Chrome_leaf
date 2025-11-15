#!/usr/bin/env python3
"""
detect_youtube.py

Passive DNS watcher that looks for DNS queries containing a target domain
(e.g. youtube.com). When detected, it triggers a local action:
  - open (open a Rickroll in the local default browser)
  - webhook (POST to a webhook URL you provide)
  - print (just log to console)

Run only on networks/devices you own or have permission to monitor.
"""

import argparse
import time
import webbrowser
import threading
from scapy.all import sniff, DNSQR, UDP, IP
import sys

try:
    import requests
except Exception:
    requests = None

RICKROLL_URL = "https://www.youtube.com/watch?v=dQw4w9WgXcQ"

def handle_action(action, src_ip, qname, webhook_url=None):
    ts = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime())
    msg = f"[{ts}] Detected DNS query from {src_ip}: {qname}"
    if action == "print":
        print(msg)
    elif action == "open":
        print(msg + " -> opening Rickroll in local browser...")
        # open in a separate thread so sniffing isn't blocked
        threading.Thread(target=webbrowser.open, args=(RICKROLL_URL,), daemon=True).start()
    elif action == "webhook":
        if requests is None:
            print("requests package not installed; cannot POST webhook. Install with `pip install requests`")
            return
        if not webhook_url:
            print("No webhook URL provided.")
            return
        payload = {"time": ts, "src_ip": src_ip, "qname": qname}
        try:
            r = requests.post(webhook_url, json=payload, timeout=5)
            print(msg + f" -> webhook posted, status {r.status_code}")
        except Exception as e:
            print(msg + " -> webhook failed:", e)
    else:
        print(msg + " -> unknown action")

def dns_pkt_callback(pkt, target_substr, action, webhook_url):
    # Filter for UDP DNS queries with DNSQR layer
    if pkt.haslayer(UDP) and pkt.haslayer(DNSQR):
        try:
            dnsq = pkt[DNSQR].qname.decode() if isinstance(pkt[DNSQR].qname, bytes) else str(pkt[DNSQR].qname)
        except Exception:
            dnsq = str(pkt[DNSQR].qname)
        # normalize and check substring
        if target_substr.lower() in dnsq.lower():
            # get source IP if available
            src_ip = pkt[IP].src if pkt.haslayer(IP) else "unknown"
            handle_action(action, src_ip, dnsq, webhook_url)

def main():
    parser = argparse.ArgumentParser(description="Passive DNS watcher for target domain substrings.")
    parser.add_argument("-i", "--iface", help="Network interface to sniff (e.g. wlan0). If omitted, Scapy chooses one.", default=None)
    parser.add_argument("-t", "--target", help="Substring to watch for in DNS queries (default: youtube.com).", default="youtube.com")
    parser.add_argument("-a", "--action", help="Action to take on detection: print | open | webhook", choices=["print", "open", "webhook"], default="print")
    parser.add_argument("-w", "--webhook", help="Webhook URL (required if action=webhook)", default=None)
    parser.add_argument("-c", "--count", type=int, help="Stop after this many matches (0 = run forever)", default=0)
    args = parser.parse_args()

    if args.action == "webhook" and not args.webhook:
        print("Error: webhook URL required when action=webhook")
        sys.exit(1)

    print("Starting passive DNS watcher.")
    print(f"Interface: {args.iface or 'default'}, target: '{args.target}', action: {args.action}")
    print("Press Ctrl+C to stop.\n")

    # scapy sniff - apply BPF filter to reduce kernel load (udp port 53)
    bpf = "udp port 53"

    # wrapper to pass extra args to callback
    def _cb(pkt):
        dns_pkt_callback(pkt, args.target, args.action, args.webhook)

    try:
        sniff(prn=_cb, filter=bpf, iface=args.iface, store=False, stop_filter=(lambda x: False if args.count == 0 else False))
    except KeyboardInterrupt:
        print("\nStopped by user.")
    except Exception as e:
        print("Error running sniff():", e)
        print("On some systems you may need to run with sudo/administrator privileges.")

if __name__ == "__main__":
    main()
