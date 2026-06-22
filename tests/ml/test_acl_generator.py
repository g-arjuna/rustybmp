"""Tests for bmppy/rbmppy/acl_generator.py (RV9-NEW3)"""
import pytest
from bmppy.rbmppy.acl_generator import AclGenerator


@pytest.fixture
def gen():
    return AclGenerator(policy_name="RUSTYBMP-BLOCK")


class TestPrefixFilter:
    def test_iosxr_prefix_set_generated(self, gen):
        out = gen.generate_prefix_filter(["203.0.113.0/24"], nos="iosxr")
        assert "prefix-set RUSTYBMP-BLOCK" in out["iosxr"]
        assert "203.0.113.0/24" in out["iosxr"]

    def test_frr_prefix_list_generated(self, gen):
        out = gen.generate_prefix_filter(["198.51.100.0/24", "192.0.2.0/24"], nos="frr")
        cfg = out["frr"]
        assert "ip prefix-list RUSTYBMP-BLOCK" in cfg
        assert "deny" in cfg
        assert "198.51.100.0/24" in cfg

    def test_junos_prefix_set_generated(self, gen):
        out = gen.generate_prefix_filter(["10.0.0.0/8"], nos="junos")
        cfg = out["junos"]
        assert "prefix-list RUSTYBMP-BLOCK" in cfg
        assert "reject" in cfg

    def test_arista_prefix_list_generated(self, gen):
        out = gen.generate_prefix_filter(["172.16.0.0/12"], nos="arista")
        cfg = out["arista"]
        assert "ip prefix-list RUSTYBMP-BLOCK" in cfg

    def test_all_nos_returned_when_nos_none(self, gen):
        out = gen.generate_prefix_filter(["203.0.113.0/24"])
        assert set(out.keys()) == {"iosxr", "frr", "junos", "arista"}

    def test_permit_action(self, gen):
        out = gen.generate_prefix_filter(["203.0.113.0/24"], action="permit", nos="frr")
        assert "permit" in out["frr"]

    def test_empty_prefix_list(self, gen):
        out = gen.generate_prefix_filter([], nos="frr")
        assert "ip prefix-list" in out["frr"]


class TestAsPathFilter:
    def test_iosxr_aspath_generated(self, gen):
        out = gen.generate_as_path_filter([64512, 64513], nos="iosxr")
        cfg = out["iosxr"]
        assert "as-path-set RUSTYBMP-BLOCK-ASPATH" in cfg
        assert "64512" in cfg

    def test_frr_aspath_generated(self, gen):
        out = gen.generate_as_path_filter([65001], nos="frr")
        cfg = out["frr"]
        assert "ip as-path access-list" in cfg
        assert "65001" in cfg

    def test_junos_aspath_generated(self, gen):
        out = gen.generate_as_path_filter([64512], nos="junos")
        cfg = out["junos"]
        assert "as-path" in cfg
        assert "64512" in cfg

    def test_arista_aspath_generated(self, gen):
        out = gen.generate_as_path_filter([64512], nos="arista")
        cfg = out["arista"]
        assert "ip as-path access-list" in cfg

    def test_all_nos_aspath(self, gen):
        out = gen.generate_as_path_filter([65000])
        assert set(out.keys()) == {"iosxr", "frr", "junos", "arista"}


class TestNullRoute:
    def test_iosxr_null_route(self, gen):
        out = gen.generate_null_route(["203.0.113.0/24"], nos="iosxr")
        assert "Null0" in out["iosxr"]
        assert "203.0.113.0/24" in out["iosxr"]

    def test_frr_null_route(self, gen):
        out = gen.generate_null_route(["203.0.113.0/24"], nos="frr")
        assert "Null0" in out["frr"]

    def test_junos_discard(self, gen):
        out = gen.generate_null_route(["203.0.113.0/24"], nos="junos")
        assert "discard" in out["junos"]

    def test_arista_null_route(self, gen):
        out = gen.generate_null_route(["203.0.113.0/24"], nos="arista")
        assert "Null0" in out["arista"]
