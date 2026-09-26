"""Exercise publication checks against an actual temporary Git index."""
from pathlib import Path
import os
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class PublicTreeChecks(unittest.TestCase):
    def test_forced_private_files_are_refused_despite_ignore_rules(self):
        with tempfile.TemporaryDirectory(prefix="nh-public-tree-") as directory:
            repo = Path(directory)
            env = {**os.environ, "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull}

            def git(*args):
                subprocess.run(["git", *args], cwd=repo, env=env, capture_output=True, check=True)

            def check():
                return subprocess.run([sys.executable, str(ROOT / "scripts/check-public-tree.py")],
                                      cwd=repo, env=env, capture_output=True, text=True)

            git("init", "--quiet")
            (repo / ".gitignore").write_bytes((ROOT / ".gitignore").read_bytes())
            (repo / "README.md").write_text("Public documentation\n", encoding="utf-8")
            git("add", ".gitignore", "README.md")
            self.assertEqual(check().returncode, 0)

            private = [".env.local", ".NoSiS/sessions/example.jsonl", "AGENTS.md",
                       "crates/example/CLAUDE.md", ".gitleaksignore", "target/dist/nh.exe"]
            for name in private:
                path = repo / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("Private fixture marker; no credentials\n", encoding="utf-8")
            git("add", "--force", "--", *private)
            result = check()
            self.assertEqual(result.returncode, 1)
            for name in private:
                self.assertIn(name, result.stderr)
            self.assertNotIn("Private fixture marker", result.stderr)


if __name__ == "__main__":
    unittest.main()
