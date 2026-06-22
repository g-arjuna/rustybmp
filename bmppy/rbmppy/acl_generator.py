"""
ACL / Prefix-List Generator (RV9-NEW3)

Given anomalous prefixes/ASNs from ML detectors, generates router ACL
configs for IOS-XR, FRRouting, JunOS, and Arista EOS.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Literal

NosType = Literal["iosxr", "frr", "junos", "arista"]


@dataclass
class AclGenerator:
    policy_name: str = "RUSTYBMP-BLOCK"

    # ── Public API ────────────────────────────────────────────────────────────

    def generate_prefix_filter(
        self,
        prefixes: list[str],
        action: str = "deny",
        nos: NosType | None = None,
    ) -> dict[str, str]:
        """
        Return vendor-specific prefix-list / prefix-set configs.

        Args:
            prefixes: list of CIDR strings, e.g. ["203.0.113.0/24"]
            action:   "deny" (null-route) or "permit"
            nos:      if set, return only that NOS; else return all four

        Returns:
            dict keyed by NOS name → config text
        """
        generators = {
            "iosxr":  self._prefix_iosxr,
            "frr":    self._prefix_frr,
            "junos":  self._prefix_junos,
            "arista": self._prefix_arista,
        }
        if nos:
            return {nos: generators[nos](prefixes, action)}
        return {k: fn(prefixes, action) for k, fn in generators.items()}

    def generate_as_path_filter(
        self,
        asns: list[int],
        action: str = "deny",
        nos: NosType | None = None,
    ) -> dict[str, str]:
        """
        Return vendor-specific AS_PATH access-list configs.

        Args:
            asns:   list of origin ASNs to block
            action: "deny" or "permit"
            nos:    if set, return only that NOS
        """
        generators = {
            "iosxr":  self._aspath_iosxr,
            "frr":    self._aspath_frr,
            "junos":  self._aspath_junos,
            "arista": self._aspath_arista,
        }
        if nos:
            return {nos: generators[nos](asns, action)}
        return {k: fn(asns, action) for k, fn in generators.items()}

    def generate_null_route(
        self,
        prefixes: list[str],
        nos: NosType | None = None,
    ) -> dict[str, str]:
        """Generate static null-route (blackhole) entries for anomalous prefixes."""
        generators = {
            "iosxr":  self._null_iosxr,
            "frr":    self._null_frr,
            "junos":  self._null_junos,
            "arista": self._null_arista,
        }
        if nos:
            return {nos: generators[nos](prefixes)}
        return {k: fn(prefixes) for k, fn in generators.items()}

    # ── IOS-XR ───────────────────────────────────────────────────────────────

    def _prefix_iosxr(self, prefixes: list[str], action: str) -> str:
        lines = [f"prefix-set {self.policy_name}"]
        lines += [f"  {p}," for p in prefixes[:-1]]
        if prefixes:
            lines.append(f"  {prefixes[-1]}")
        lines.append("end-set")
        lines.append("!")
        lines.append(f"route-policy {self.policy_name}-POLICY")
        lines.append(f"  if destination in {self.policy_name} then")
        lines.append(f"    {'drop' if action == 'deny' else 'pass'}")
        lines.append("  endif")
        lines.append("end-policy")
        return "\n".join(lines)

    def _aspath_iosxr(self, asns: list[int], action: str) -> str:
        patterns = " ".join(f"_^{asn}$_" for asn in asns)
        lines = [
            f"as-path-set {self.policy_name}-ASPATH",
            f"  ios-regex '{patterns}'",
            "end-set",
            "!",
            f"route-policy {self.policy_name}-ASPATH-POLICY",
            f"  if as-path in {self.policy_name}-ASPATH then",
            f"    {'drop' if action == 'deny' else 'pass'}",
            "  endif",
            "end-policy",
        ]
        return "\n".join(lines)

    def _null_iosxr(self, prefixes: list[str]) -> str:
        lines = []
        for p in prefixes:
            lines.append(f"router static address-family ipv4 unicast {p} Null0 tag 666")
        return "\n".join(lines)

    # ── FRRouting ─────────────────────────────────────────────────────────────

    def _prefix_frr(self, prefixes: list[str], action: str) -> str:
        lines = []
        for i, p in enumerate(prefixes, start=5):
            lines.append(f"ip prefix-list {self.policy_name} seq {i * 5} {action} {p}")
        lines.append(f"ip prefix-list {self.policy_name} seq 65535 permit 0.0.0.0/0 le 32")
        return "\n".join(lines)

    def _aspath_frr(self, asns: list[int], action: str) -> str:
        lines = []
        for i, asn in enumerate(asns, start=1):
            lines.append(f"ip as-path access-list {self.policy_name}-ASPATH seq {i * 5} {action} _{asn}_")
        return "\n".join(lines)

    def _null_frr(self, prefixes: list[str]) -> str:
        lines = []
        for p in prefixes:
            lines.append(f"ip route {p} Null0 tag 666")
        return "\n".join(lines)

    # ── JunOS ─────────────────────────────────────────────────────────────────

    def _prefix_junos(self, prefixes: list[str], action: str) -> str:
        j_action = "reject" if action == "deny" else "accept"
        lines = [f"policy-options {{"]
        lines.append(f"    prefix-list {self.policy_name} {{")
        for p in prefixes:
            lines.append(f"        {p};")
        lines.append("    }")
        lines.append(f"    policy-statement {self.policy_name}-POLICY {{")
        lines.append("        term MATCH {")
        lines.append("            from {")
        lines.append(f"                prefix-list {self.policy_name};")
        lines.append("            }")
        lines.append(f"            then {j_action};")
        lines.append("        }")
        lines.append("    }")
        lines.append("}")
        return "\n".join(lines)

    def _aspath_junos(self, asns: list[int], action: str) -> str:
        j_action = "reject" if action == "deny" else "accept"
        patterns = "|".join(str(asn) for asn in asns)
        lines = [
            "policy-options {",
            f"    as-path {self.policy_name}-ASPATH \".* ({patterns})\";",
            f"    policy-statement {self.policy_name}-ASPATH-POLICY {{",
            "        term MATCH {",
            "            from {",
            f"                as-path {self.policy_name}-ASPATH;",
            "            }",
            f"            then {j_action};",
            "        }",
            "    }",
            "}",
        ]
        return "\n".join(lines)

    def _null_junos(self, prefixes: list[str]) -> str:
        lines = ["routing-options {", "    static {"]
        for p in prefixes:
            lines.append(f"        route {p} discard;")
        lines += ["    }", "}"]
        return "\n".join(lines)

    # ── Arista EOS ────────────────────────────────────────────────────────────

    def _prefix_arista(self, prefixes: list[str], action: str) -> str:
        lines = []
        for i, p in enumerate(prefixes, start=1):
            lines.append(f"ip prefix-list {self.policy_name} seq {i * 10} {action} {p}")
        lines.append(f"!")
        lines.append(f"route-map {self.policy_name}-MAP deny 10")
        lines.append(f"   match ip address prefix-list {self.policy_name}")
        lines.append(f"!")
        lines.append(f"route-map {self.policy_name}-MAP permit 20")
        return "\n".join(lines)

    def _aspath_arista(self, asns: list[int], action: str) -> str:
        lines = []
        for i, asn in enumerate(asns, start=1):
            lines.append(f"ip as-path access-list {self.policy_name}-ASPATH {i * 10} {action} _{asn}_")
        return "\n".join(lines)

    def _null_arista(self, prefixes: list[str]) -> str:
        lines = []
        for p in prefixes:
            lines.append(f"ip route {p} Null0 tag 666")
        return "\n".join(lines)
