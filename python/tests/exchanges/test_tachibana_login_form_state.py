import pytest
from engine.exchanges.tachibana_login_form_state import (
    build_form_init, validate_submission, EMPTY_FIELDS,
)


def test_build_form_init_allow_prod_true():
    fi = build_form_init("prod", env_dict={"TACHIBANA_ALLOW_PROD": "1"}, is_debug_build=False)
    assert fi.allow_prod is True


def test_build_form_init_release_no_dev_env():
    fi = build_form_init("demo", env_dict={"DEV_TACHIBANA_USER_ID": "u", "DEV_TACHIBANA_PASSWORD": "p"}, is_debug_build=False)
    assert fi.dev_user_id is None
    assert fi.dev_password is None
    assert fi.dev_demo is None


def test_build_form_init_prod_hint_with_allow_prod():
    fi = build_form_init("prod", env_dict={"TACHIBANA_ALLOW_PROD": "1"}, is_debug_build=True)
    assert fi.initial_mode == "prod"


def test_build_form_init_prod_hint_without_allow_prod():
    fi = build_form_init("prod", env_dict={}, is_debug_build=True)
    assert fi.initial_mode == "demo"


def test_validate_submission_empty_user_id():
    assert validate_submission("", "pass", "demo") == EMPTY_FIELDS


def test_validate_submission_empty_password():
    assert validate_submission("user", "", "demo") == EMPTY_FIELDS


def test_validate_submission_valid():
    assert validate_submission("user", "pass", "demo") is None
