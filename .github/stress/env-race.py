"""Changes the environment from Python while Qt WebEngine starts its GPU thread (#1113).

If the crash is glibc setenv racing getenv on WebEngine's GPU thread, this dies with SIGSEGV;
without the setenv loop (ENV_RACE=0) it exits cleanly.
"""

import os
import time

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
from PySide6.QtCore import QUrl  # noqa: E402
from PySide6.QtWebEngineWidgets import QWebEngineView  # noqa: E402
from PySide6.QtWidgets import QApplication  # noqa: E402

app = QApplication([])
views = []
end = time.monotonic() + 20
i = 0
while time.monotonic() < end:
    if len(views) < 4:
        view = QWebEngineView()
        view.setHtml("<p>x</p>", QUrl("https://example.invalid/"))
        views.append(view)
    if os.environ.get("ENV_RACE", "1") == "1":
        for j in range(200):
            os.environ[f"RUSTGIS_RACE_{j}"] = str(i)
        for j in range(200):
            del os.environ[f"RUSTGIS_RACE_{j}"]
    app.processEvents()
    i += 1
print(f"survived {i} rounds", flush=True)
