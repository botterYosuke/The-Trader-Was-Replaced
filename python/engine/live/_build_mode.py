"""Build-mode flag: True in development/debug builds, False in release.

Release pipelines run tools/freeze_build_mode.py --release before packaging.
"""
IS_DEBUG_BUILD = True
