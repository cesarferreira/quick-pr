"""Exercise release boundaries in isolated repositories without publishing."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'scripts').mkdir()
        for name in ['release.sh', 'release-changelog.sh']:
            shutil.copy(Path(__file__).parent / name, self.root / 'scripts' / name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.log = self.root / '.git' / 'commands'
        self.env = dict(os.environ, PATH=f'{self.bin}:{os.environ["PATH"]}',
                        RELEASE_TEST_LOG=str(self.log), VISIBILITY='PUBLIC')
        self.run_git('init', '-b', 'main')
        (self.root / '.git' / 'info' / 'exclude').write_text('bin/\n')
        self.run_git('add', '.')
        self.run_git('-c', 'user.name=Test', '-c', 'user.email=test@example.com',
                     'commit', '-m', 'Initial')
        self.stub('uname', 'echo Darwin')
        self.stub('gh', 'echo "$VISIBILITY"')
        self.stub('make', 'echo "make $*" >> "$RELEASE_TEST_LOG"')
        self.stub('cargo', 'echo "cargo $*" >> "$RELEASE_TEST_LOG"')
        self.stub('git-cliff', 'echo "cliff $*" >> "$RELEASE_TEST_LOG"; echo changelog')

    def run_git(self, *args):
        subprocess.run(['git', *args], cwd=self.root, check=True, capture_output=True)

    def stub(self, name, body):
        path = self.bin / name
        path.write_text('#!/bin/bash\n' + body + '\n')
        path.chmod(0o755)

    def release(self, *args):
        return subprocess.run(['bash', 'scripts/release.sh', *args], cwd=self.root,
                              env=self.env, capture_output=True, text=True)

    def commands(self):
        return self.log.read_text() if self.log.exists() else ''

    def test_dry_run_never_adds_execute_or_requires_public_repo(self):
        self.env['VISIBILITY'] = 'PRIVATE'
        result = self.release('patch')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.commands(), 'make test check\ncargo release patch\n')

    def test_private_repo_blocks_publication(self):
        self.env['VISIBILITY'] = 'PRIVATE'
        result = self.release('minor', '--execute')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Repository is private', result.stderr)
        self.assertEqual(self.commands(), '')

    def test_execute_runs_checks_before_publication(self):
        result = self.release('release', '--execute')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.commands(),
                         'make test check\ncargo release release --execute --no-confirm\n')

    def test_failed_checks_stop_publication(self):
        self.stub('make', 'exit 1')
        result = self.release('patch', '--execute')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.commands(), '')

    def test_dirty_tree_stops_before_checks(self):
        (self.root / 'uncommitted').write_text('change')
        result = self.release('patch')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Commit all changes', result.stderr)
        self.assertEqual(self.commands(), '')

    def test_dry_run_hook_preserves_changelog(self):
        changelog = self.root / 'CHANGELOG.md'
        changelog.write_text('original')
        result = subprocess.run(['bash', 'scripts/release-changelog.sh'], cwd=self.root,
                                env=dict(self.env, DRY_RUN='true', NEW_VERSION='0.2.0'),
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(changelog.read_text(), 'original')
        self.assertNotIn('--output', self.commands())


if __name__ == '__main__':
    unittest.main()
