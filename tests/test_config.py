"""Minimal smoke-tests for ProxyServerConfig — no live backend required."""

import pytest
import object_storage_proxy as osp


def test_proxy_server_config_can_be_instantiated():
    """ProxyServerConfig can be created with a minimal cos_map."""
    cfg = osp.ProxyServerConfig(cos_map={})
    assert cfg is not None


def test_proxy_server_config_default_ports_are_none():
    cfg = osp.ProxyServerConfig(cos_map={})
    assert cfg.http_port is None
    assert cfg.https_port is None


def test_proxy_server_config_custom_ports():
    cfg = osp.ProxyServerConfig(cos_map={}, http_port=8080, https_port=8443)
    assert cfg.http_port == 8080
    assert cfg.https_port == 8443


def test_proxy_server_config_default_threads():
    cfg = osp.ProxyServerConfig(cos_map={})
    assert cfg.threads == 1


def test_proxy_server_config_custom_server_name():
    cfg = osp.ProxyServerConfig(cos_map={}, server_name="my-proxy")
    assert cfg.server_name == "my-proxy"


def test_proxy_server_config_default_server_name():
    cfg = osp.ProxyServerConfig(cos_map={})
    assert cfg.server_name == "<osp⚡>"


def test_proxy_server_config_skip_signature_validation_default():
    cfg = osp.ProxyServerConfig(cos_map={})
    assert cfg.skip_signature_validation is False


def test_proxy_server_config_max_presign_url_usage_attempts_default():
    cfg = osp.ProxyServerConfig(cos_map={})
    assert cfg.max_presign_url_usage_attempts == 3


def test_proxy_server_config_repr_contains_port_info():
    cfg = osp.ProxyServerConfig(cos_map={}, http_port=9000, https_port=9443, threads=2)
    r = repr(cfg)
    assert "9000" in r
    assert "9443" in r
    assert "2" in r


def test_cos_map_item_is_exported():
    """CosMapItem class is accessible in the module."""
    assert hasattr(osp, "CosMapItem")


def test_module_has_start_server():
    assert callable(osp.start_server)
