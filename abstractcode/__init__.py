"""
AbstractCode - A clean terminal CLI for multi-agent agentic coding

This package provides a terminal-based interface for AI-powered coding assistance
using multiple coordinated agents. Built on the Abstract Framework ecosystem.

Author: Laurent-Philippe Albou
Email: contact@abstractcore.ai
Website: https://abstractcore.ai
"""

__version__ = "0.2.0"
__author__ = "Laurent-Philippe Albou"
__email__ = "contact@abstractcore.ai"
__license__ = "MIT"

def main(argv=None):
    """Console entrypoint for `abstractcode`."""
    from .cli import main as _main

    return _main(argv)

__all__ = ["__version__", "__author__", "__email__", "__license__", "main"]
