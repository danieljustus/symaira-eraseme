from __future__ import annotations

import json
import urllib.error
from unittest.mock import patch

import pytest

from symeraseme.llm.ollama_client import OllamaClient
from symeraseme.llm.protocol import LLMClientError, UsageRecord


class MockHTTPResponse:
    """Mock for urllib.request.urlopen response."""

    def __init__(self, body: bytes, status: int = 200):
        self._body = body
        self.status = status

    def read(self):
        return self._body

    def __enter__(self):
        return self

    def __exit__(self, *args):
        pass


class TestOllamaClientIsAvailable:
    def test_available_when_model_present(self):
        response_body = json.dumps(
            {
                "models": [
                    {"name": "llama3.1", "model": "llama3.1:latest"},
                    {"name": "mistral", "model": "mistral:latest"},
                ]
            }
        ).encode("utf-8")

        with patch("urllib.request.urlopen", return_value=MockHTTPResponse(response_body)):
            client = OllamaClient(model="llama3.1")
            assert client.is_available()

    def test_available_when_model_matches_model_field(self):
        response_body = json.dumps(
            {
                "models": [
                    {"name": "llama3.1:latest", "model": "llama3.1:latest"},
                ]
            }
        ).encode("utf-8")

        with patch("urllib.request.urlopen", return_value=MockHTTPResponse(response_body)):
            client = OllamaClient(model="llama3.1:latest")
            assert client.is_available()

    def test_not_available_when_model_missing(self):
        response_body = json.dumps(
            {
                "models": [
                    {"name": "mistral", "model": "mistral:latest"},
                ]
            }
        ).encode("utf-8")

        with patch("urllib.request.urlopen", return_value=MockHTTPResponse(response_body)):
            client = OllamaClient(model="llama3.1")
            assert not client.is_available()

    def test_not_available_on_connection_error(self):
        with patch(
            "urllib.request.urlopen",
            side_effect=urllib.error.URLError("Connection refused"),
        ):
            client = OllamaClient()
            assert not client.is_available()

    def test_not_available_on_timeout(self):
        with patch("urllib.request.urlopen", side_effect=TimeoutError("timed out")):
            client = OllamaClient()
            assert not client.is_available()

    def test_not_available_on_bad_json(self):
        with patch("urllib.request.urlopen", return_value=MockHTTPResponse(b"not json")):
            client = OllamaClient()
            assert not client.is_available()

    def test_not_available_on_non_200_status(self):
        with patch("urllib.request.urlopen", return_value=MockHTTPResponse(b"", status=500)):
            client = OllamaClient()
            assert not client.is_available()

    def test_custom_host(self):
        response_body = json.dumps(
            {"models": [{"name": "llama3.1", "model": "llama3.1:latest"}]}
        ).encode("utf-8")

        with patch("urllib.request.urlopen") as mock_urlopen:
            mock_urlopen.return_value = MockHTTPResponse(response_body)
            client = OllamaClient(host="http://192.168.1.100:11434", model="llama3.1")
            result = client.is_available()

        assert result
        call_args = mock_urlopen.call_args
        req = call_args[0][0]
        assert "192.168.1.100:11434/api/tags" in req.full_url


class TestOllamaClientClassify:
    def test_classify_success(self):
        response_body = json.dumps(
            {
                "model": "llama3.1",
                "message": {"role": "assistant", "content": "positive"},
                "prompt_eval_count": 25,
                "eval_count": 8,
            }
        ).encode("utf-8")

        with patch("urllib.request.urlopen", return_value=MockHTTPResponse(response_body)):
            client = OllamaClient()
            text, record = client._call_api(
                system_prompt="You are a classifier.",
                user_prompt="Classify this.",
                max_tokens=512,
                temperature=0.0,
                cache_key=None,
            )

        assert text == "positive"
        assert isinstance(record, UsageRecord)
        assert record.input_tokens == 25
        assert record.output_tokens == 8
        assert record.model == "llama3.1"
        assert record.cost == 0.0

    def test_classify_with_custom_model(self):
        response_body = json.dumps(
            {
                "model": "mistral",
                "message": {"role": "assistant", "content": "negative"},
                "prompt_eval_count": 30,
                "eval_count": 12,
            }
        ).encode("utf-8")

        with patch("urllib.request.urlopen", return_value=MockHTTPResponse(response_body)):
            client = OllamaClient(model="mistral")
            text, record = client._call_api(
                system_prompt="sys",
                user_prompt="user",
                max_tokens=512,
                temperature=0.0,
                cache_key=None,
            )

        assert text == "negative"
        assert record.model == "mistral"

    def test_classify_payload_structure(self):
        response_body = json.dumps(
            {
                "model": "llama3.1",
                "message": {"role": "assistant", "content": "ok"},
                "prompt_eval_count": 10,
                "eval_count": 2,
            }
        ).encode("utf-8")

        with patch("urllib.request.urlopen") as mock_urlopen:
            mock_urlopen.return_value = MockHTTPResponse(response_body)
            client = OllamaClient()
            client._call_api(
                system_prompt="You are a classifier.",
                user_prompt="Classify this.",
                max_tokens=256,
                temperature=0.5,
                cache_key=None,
            )

        call_args = mock_urlopen.call_args
        req = call_args[0][0]
        payload = json.loads(req.data)

        assert payload["model"] == "llama3.1"
        assert payload["stream"] is False
        assert payload["options"]["temperature"] == 0.5
        assert payload["options"]["num_predict"] == 256
        assert payload["messages"][0]["role"] == "system"
        assert payload["messages"][0]["content"] == "You are a classifier."
        assert payload["messages"][1]["role"] == "user"
        assert payload["messages"][1]["content"] == "Classify this."

    def test_classify_empty_response(self):
        response_body = json.dumps(
            {
                "model": "llama3.1",
                "message": {},
                "prompt_eval_count": 0,
                "eval_count": 0,
            }
        ).encode("utf-8")

        with patch("urllib.request.urlopen", return_value=MockHTTPResponse(response_body)):
            client = OllamaClient()
            text, record = client._call_api(
                system_prompt="sys",
                user_prompt="user",
                max_tokens=512,
                temperature=0.0,
                cache_key=None,
            )

        assert text == ""
        assert record.input_tokens == 0
        assert record.output_tokens == 0


class TestOllamaClientCost:
    def test_cost_is_always_zero(self):
        response_body = json.dumps(
            {
                "model": "llama3.1",
                "message": {"role": "assistant", "content": "test"},
                "prompt_eval_count": 1000,
                "eval_count": 500,
            }
        ).encode("utf-8")

        with patch("urllib.request.urlopen", return_value=MockHTTPResponse(response_body)):
            client = OllamaClient()
            text, record = client._call_api(
                system_prompt="sys",
                user_prompt="user",
                max_tokens=512,
                temperature=0.0,
                cache_key=None,
            )

        assert record.cost == 0.0

    def test_cost_tracker_appends(self):
        response_body = json.dumps(
            {
                "model": "llama3.1",
                "message": {"role": "assistant", "content": "test"},
                "prompt_eval_count": 100,
                "eval_count": 50,
            }
        ).encode("utf-8")

        tracker: list[UsageRecord] = []
        with patch("urllib.request.urlopen", return_value=MockHTTPResponse(response_body)):
            client = OllamaClient(cost_tracker=tracker)
            client._call_api(
                system_prompt="sys",
                user_prompt="user",
                max_tokens=512,
                temperature=0.0,
                cache_key=None,
            )

        assert len(tracker) == 1
        assert tracker[0].cost == 0.0
        assert tracker[0].input_tokens == 100
        assert tracker[0].output_tokens == 50


class TestOllamaClientTimeout:
    def test_classify_raises_on_timeout(self):
        with patch("urllib.request.urlopen", side_effect=TimeoutError("timed out")):
            client = OllamaClient()
            with pytest.raises(LLMClientError, match="connection error"):
                client._call_api(
                    system_prompt="sys",
                    user_prompt="user",
                    max_tokens=512,
                    temperature=0.0,
                    cache_key=None,
                )

    def test_classify_raises_on_url_error(self):
        with patch(
            "urllib.request.urlopen",
            side_effect=urllib.error.URLError("Connection refused"),
        ):
            client = OllamaClient()
            with pytest.raises(LLMClientError, match="connection error"):
                client._call_api(
                    system_prompt="sys",
                    user_prompt="user",
                    max_tokens=512,
                    temperature=0.0,
                    cache_key=None,
                )

    def test_classify_raises_on_http_error(self):
        error = urllib.error.HTTPError(
            url="http://localhost:11434/api/chat",
            code=404,
            msg="Not Found",
            hdrs={},
            fp=None,
        )
        with patch("urllib.request.urlopen", side_effect=error):
            client = OllamaClient()
            with pytest.raises(LLMClientError, match="HTTP error 404"):
                client._call_api(
                    system_prompt="sys",
                    user_prompt="user",
                    max_tokens=512,
                    temperature=0.0,
                    cache_key=None,
                )

    def test_classify_raises_on_bad_json(self):
        with patch("urllib.request.urlopen", return_value=MockHTTPResponse(b"not json")):
            client = OllamaClient()
            with pytest.raises(LLMClientError, match="Invalid JSON"):
                client._call_api(
                    system_prompt="sys",
                    user_prompt="user",
                    max_tokens=512,
                    temperature=0.0,
                    cache_key=None,
                )
