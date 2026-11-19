# dmgbuild settings for the Battlefront Eternal disk image (build/native/package-macos.py).
# The window's layout is written straight into the image's .DS_Store, so the image can be
# built without a desktop session.
import os.path

app = defines['app']  # noqa: F821 (dmgbuild provides `defines`)
appname = os.path.basename(app)

format = 'ULFO'
files = [app]
symlinks = {'Applications': '/Applications'}
icon = defines.get('icon')  # noqa: F821
background = defines['background']  # noqa: F821

default_view = 'icon-view'
window_rect = ((200, 140), (660, 420))
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
arrange_by = None
icon_size = 128
text_size = 13
icon_locations = {appname: (180, 185), 'Applications': (480, 185)}
