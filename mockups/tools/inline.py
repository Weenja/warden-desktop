#!/usr/bin/env python3
"""Bundle a mockup page into one self-contained HTML file.

Usage: python3 -I tools/inline.py extension/index.html  ->  extension/index.bundle.html
Inlines local stylesheets, scripts, images and fonts as data URIs so the page
opens anywhere (file card, artifact, email) without its folder.
"""
import base64, mimetypes, pathlib, re, sys

mimetypes.add_type("font/ttf", ".ttf")
mimetypes.add_type("image/webp", ".webp")
mimetypes.add_type("image/svg+xml", ".svg")

def local(ref):
    return not re.match(r"^(https?:|data:|#|mailto:|javascript:)", ref)

def data_uri(path):
    mime = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
    return f"data:{mime};base64,{base64.b64encode(path.read_bytes()).decode()}"

def fix_css(css, base):
    def sub(m):
        ref = m.group(2)
        return f"url({data_uri((base / ref).resolve())})" if local(ref) else m.group(0)
    return re.sub(r"url\((['\"]?)([^)'\"]+)\1\)", sub, css)

def bundle(page):
    base = page.parent
    html = page.read_text()

    def link(m):
        href = re.search(r'href="([^"]+)"', m.group(0)).group(1)
        if not local(href):
            return m.group(0)
        css_path = (base / href).resolve()
        return f"<style>{fix_css(css_path.read_text(), css_path.parent)}</style>"
    html = re.sub(r'<link[^>]*rel="stylesheet"[^>]*>', link, html)

    def icon(m):
        href = m.group(2)
        return f'{m.group(1)}href="{data_uri((base / href).resolve())}"' if local(href) else m.group(0)
    html = re.sub(r'(<link[^>]*rel="icon"[^>]*?)href="([^"]+)"', icon, html)

    def script(m):
        src = m.group(1)
        return f"<script>{(base / src).read_text()}</script>" if local(src) else m.group(0)
    html = re.sub(r'<script[^>]*src="([^"]+)"[^>]*></script>', script, html)

    def attr(m):
        ref = m.group(2)
        return f'{m.group(1)}="{data_uri((base / ref).resolve())}"' if local(ref) else m.group(0)
    html = re.sub(r'\b(src|poster)="([^"]+)"', attr, html)

    # url() inside inline <style> blocks and style="" attributes
    html = fix_css(html, base)
    out = page.with_name(page.stem + ".bundle.html")
    out.write_text(html)
    print(out, f"{out.stat().st_size // 1024} KB")

for arg in sys.argv[1:]:
    bundle(pathlib.Path(arg))
