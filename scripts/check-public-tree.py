"""Reject private working material and generated artifacts in the Git index.

Run after staging and before committing. This checks paths, not secret contents;
use a secret scanner as well. It never deletes or changes files.
"""
from pathlib import PurePosixPath
import subprocess
import sys


PRIVATE_DIRS = {
    ".nosis", ".claude", ".codex", ".agents", ".venv", "venv", "__pycache__",
    ".pytest_cache", "target", "00-start-here", "01-product", "02-architecture",
    "03-execution", "04-research", "05-ai-collaboration", "06-operations",
    "07-assets", "08-decisions-and-risk", "09-customer-learning",
    "10-knowledge-system", "11-automation", "12-executive",
}
PRIVATE_DOC_FILES = {
    "agents.md", "claude.md", "codex.md", "brief_m2.md",
    "contracts_m1.md", "contracts_m2.md", "contracts_m3.md", "contracts_m4.md",
    "contracts_m5.md", "nosis_harness_master_plan.md",
}
PRIVATE_SUFFIXES = {
    ".key", ".pem", ".p12", ".pfx", ".jks", ".exe", ".dll", ".pdb",
    ".dmp", ".zip", ".pyc", ".pyo", ".pyd",
}


def forbidden_path(name):
    path = PurePosixPath(name.lower())
    return (
        bool(PRIVATE_DIRS.intersection(path.parts[:-1]))
        or path.name in PRIVATE_DOC_FILES
        or path.name == ".env"
        or path.name.startswith(".env.")
        or path.name in {"id_rsa", "id_ed25519", ".ds_store", "thumbs.db", ".gitleaksignore"}
        or path.suffix in PRIVATE_SUFFIXES
    )


def main():
    result = subprocess.run(
        ["git", "ls-files", "--cached", "-z"], capture_output=True, check=True
    )
    names = [name for name in result.stdout.decode("utf-8").split("\0") if name]
    rejected = sorted(name for name in names if forbidden_path(name))
    if rejected:
        print("Private or generated paths in the Git index:", file=sys.stderr)
        for name in rejected:
            print(f"  {name}", file=sys.stderr)
        return 1
    print(f"Public-tree path check passed ({len(names)} indexed files).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
