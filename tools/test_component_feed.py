import unittest

from component_feed import valid_artifact_url


class ArtifactUrlTests(unittest.TestCase):
    def test_rejects_urls_the_desktop_verifier_cannot_use(self):
        for url in (
            "https://updates.example.org/package.zip?",
            "https://updates.example.org/package.zip#",
            "https://updates.example.org/package.zip?mirror=one",
            "https://updates.example.org/package.zip#archive",
            "https://xn--a.example.test/package.zip",
            "https://[v1.abc]/package.zip",
            "https://[::1]suffix/package.zip",
        ):
            with self.subTest(url=url):
                self.assertFalse(valid_artifact_url(url))

    def test_accepts_https_archives(self):
        for url in (
            "https://updates.example.org/package%3F%23.zip",
            "https://xn--bcher-kva.example/package.zip",
            "https://[::1]:8443/package.zip",
        ):
            with self.subTest(url=url):
                self.assertTrue(valid_artifact_url(url))
