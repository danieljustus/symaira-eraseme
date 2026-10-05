import io
import os
import unittest
import urllib.error
import urllib.request
from unittest.mock import patch

import prepare_backup_restore as prepare


class MetadataCredentialScope(unittest.TestCase):
    def test_only_exact_metadata_request_receives_token(self):
        requests = []

        def open_request(request, timeout):
            requests.append(request)
            self.assertEqual(timeout, 30)
            return io.BytesIO(b'{}')

        with patch.dict(os.environ, {'ERASEME_RELEASE_METADATA_TOKEN': 'synthetic-test-token'}), \
                patch('urllib.request.OpenerDirector.open', side_effect=open_request), \
                patch('urllib.request.urlopen', side_effect=open_request):
            prepare.fetch(prepare.RELEASE_METADATA_URL, 32)
            prepare.fetch('https://github.com/' + prepare.REPOSITORY + '/releases/download/v0.12.1/checksums.txt', 32)
            prepare.fetch('https://api.github.com/repos/example/other', 32)
        self.assertEqual(requests[0].get_header('Authorization'), 'Bearer synthetic-test-token')
        self.assertIsNone(requests[1].get_header('Authorization'))
        self.assertIsNone(requests[2].get_header('Authorization'))

    def test_authenticated_redirect_is_refused(self):
        request = urllib.request.Request(prepare.RELEASE_METADATA_URL,
                                         headers={'Authorization': 'Bearer synthetic-test-token'})
        for target in ('https://api.github.com/other', 'https://github.com/asset', 'https://example.invalid/'):
            with self.subTest(target=target), self.assertRaises(urllib.error.HTTPError):
                prepare.NoCredentialRedirect().redirect_request(request, io.BytesIO(), 302, '', {}, target)

    def test_anonymous_download_retains_response_bound(self):
        with patch.dict(os.environ, {}, clear=True), \
                patch('urllib.request.urlopen', return_value=io.BytesIO(b'12345')):
            with self.assertRaisesRegex(ValueError, 'release response exceeded its byte bound'):
                prepare.fetch(prepare.RELEASE_METADATA_URL, 4)


if __name__ == '__main__':
    unittest.main()
