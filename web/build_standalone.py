"""Bundle the local ES modules and CSS into a dependency-free HTML file."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent


def build():
    sources = []
    for name in ('protocol.js', 'transport.js', 'app.js'):
        source = (ROOT / name).read_text(encoding='utf-8')
        # These modules only use one-line named imports and declaration exports.
        source = re.sub(r'^import \{[^\n]+\} from [\'\"][^\'\"]+[\'\"];\n', '', source, flags=re.M)
        source = re.sub(r'^export (?=const |function |class )', '', source, flags=re.M)
        if re.search(r'^\s*(import|export)\b', source, flags=re.M):
            raise ValueError(f'Unsupported module syntax in {name}')
        sources.append(f'// {name}\n{source}')
    license_text = (ROOT / 'THIRD_PARTY_NOTICES.md').read_text(encoding='utf-8')
    script = '/*\n' + license_text.replace('*/', '* /') + '\n*/\n(() => {\n"use strict";\n' + '\n'.join(sources) + '\n})();'
    script = re.sub(r'</script', r'<\\/script', script, flags=re.I)
    css = (ROOT / 'style.css').read_text(encoding='utf-8')
    html = (ROOT / 'index.html').read_text(encoding='utf-8')
    style_tag = '<link rel="stylesheet" href="style.css">'
    script_tag = '<script type="module" src="app.js"></script>'
    if html.count(style_tag) != 1 or html.count(script_tag) != 1:
        raise ValueError('HTML entry points changed; update the bundler')
    html = html.replace(style_tag, f'<style>\n{css}\n</style>')
    html = html.replace(script_tag, f'<script>\n{script}\n</script>')
    target = ROOT / 'dm40.html'
    target.write_text(html, encoding='utf-8')
    print(f'{target} ({target.stat().st_size:,} bytes)')


if __name__ == '__main__':
    build()
