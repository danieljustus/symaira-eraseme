"""OpenEraseMe — Deprecated redirect package.

This package has been renamed to 'symeraseme'. Please update your dependencies:

    pip install symeraseme

For more information, see: https://github.com/danieljustus/symaira-eraseme
"""

import warnings

warnings.warn(
    "The package 'openeraseme' is deprecated and has been renamed to 'symeraseme'. "
    "Please update your dependencies: pip install symeraseme",
    DeprecationWarning,
    stacklevel=2,
)

# Re-export everything from symeraseme
from symeraseme import *  # noqa: F403
from symeraseme import __version__  # noqa: F401

__version__ = "0.1.4 (redirect to symeraseme)"
