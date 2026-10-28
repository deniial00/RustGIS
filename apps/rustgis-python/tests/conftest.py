"""What the tests share."""

import os
import pathlib
import shutil
import tempfile

import pytest


def pytest_configure(config):
    """Points RUSTGIS_CONFIG_DIR at a scratch folder, once, before any test runs.

    A test must not change the process environment: Qt WebEngine reads it from its own threads
    (Mesa's EGL setup calls getenv on WebEngine's GPU thread), and glibc's setenv can free the
    environment under a concurrent getenv, a segfault on that thread (#1113). Set here, the
    variable is in place before the Qt tests start any thread, and the desktop preferences the
    tests write never reach the user's own settings folder.
    """
    os.environ["RUSTGIS_CONFIG_DIR"] = tempfile.mkdtemp(prefix="rustgis-tests-")


def pytest_unconfigure(config):
    """Removes the scratch settings folder."""
    shutil.rmtree(os.environ["RUSTGIS_CONFIG_DIR"], ignore_errors=True)


@pytest.fixture
def config_dir():
    """The settings folder RustGIS reads (RUSTGIS_CONFIG_DIR), empty for this test."""
    path = pathlib.Path(os.environ["RUSTGIS_CONFIG_DIR"])

    def empty():
        for child in path.iterdir():
            if child.is_dir():
                shutil.rmtree(child)
            else:
                child.unlink()

    empty()
    yield path
    empty()
