from __future__ import annotations

from unittest.mock import patch

from symeraseme.services.reply import handle_classify_reply, handle_generate_rebuttal

SR = "symeraseme.services.reply"


class TestHandleClassifyReply:
    def test_calls_create_llm_client(self):
        with (
            patch(f"{SR}._ensure_llm_consent", return_value=None),
            patch(f"{SR}.get_removal_request", return_value={"broker_id": "test"}),
            patch(f"{SR}.get_events", return_value=[{"payload_json": {}, "occurred_at": ""}]),
            patch(f"{SR}.load_broker", return_value=None),
            patch(f"{SR}.get_connection") as conn,
            patch("symeraseme.llm.factory.create_llm_client") as mock_create,
            patch(f"{SR}.ReplyClassifier") as mock_cls,
            patch(f"{SR}.insert_inbox_reply"),
            patch(f"{SR}.append_event_and_project"),
        ):
            conn.return_value.execute.return_value.fetchone.return_value = {
                "id": 1,
                "subject": "Re: test",
                "snippet": "body",
                "from_addr": "b@b.com",
            }
            mock_cls_instance = mock_cls.return_value
            mock_cls_instance.is_available.return_value = True
            mock_cls_instance.classify.return_value.label = "ack"
            mock_cls_instance.classify.return_value.event_type = "ACK"
            mock_cls_instance.classify.return_value.confidence = 0.95
            mock_cls_instance.classify.return_value.summary = "OK"
            mock_cls_instance.classify.return_value.needs_human_review = False
            mock_cls_instance.classify.return_value.extracted_fields = {}
            mock_cls_instance.classify.return_value.usage_record = None

            handle_classify_reply(request_id=1, provider="openai", model="gpt-4o")

            mock_create.assert_called_once_with(provider="openai", model="gpt-4o")

    def test_appends_single_event_per_classification(self):
        with (
            patch(f"{SR}._ensure_llm_consent", return_value=None),
            patch(f"{SR}.get_removal_request", return_value={"broker_id": "test"}),
            patch(f"{SR}.get_events", return_value=[{"payload_json": {}, "occurred_at": ""}]),
            patch(f"{SR}.load_broker", return_value=None),
            patch(f"{SR}.get_connection") as conn,
            patch("symeraseme.llm.factory.create_llm_client"),
            patch(f"{SR}.ReplyClassifier") as mock_cls,
            patch(f"{SR}.insert_inbox_reply"),
            patch(f"{SR}.append_event_and_project") as mock_event,
        ):
            conn.return_value.execute.return_value.fetchone.return_value = {
                "id": 1,
                "subject": "Re: test",
                "snippet": "body",
                "from_addr": "b@b.com",
            }
            mock_cls_instance = mock_cls.return_value
            mock_cls_instance.is_available.return_value = True
            mock_cls_instance.classify.return_value.label = "ack"
            mock_cls_instance.classify.return_value.event_type = "ACK"
            mock_cls_instance.classify.return_value.confidence = 0.95
            mock_cls_instance.classify.return_value.summary = "OK"
            mock_cls_instance.classify.return_value.needs_human_review = False
            mock_cls_instance.classify.return_value.extracted_fields = {}
            mock_cls_instance.classify.return_value.usage_record = None

            handle_classify_reply(request_id=1)

            mock_event.assert_called_once()
            call_kwargs = mock_event.call_args.kwargs
            assert call_kwargs.get("payload", {}).get("classification") == "ack"

    def test_calls_create_llm_client_with_defaults(self):
        with (
            patch(f"{SR}._ensure_llm_consent", return_value=None),
            patch(f"{SR}.get_removal_request", return_value={"broker_id": "test"}),
            patch(f"{SR}.get_events", return_value=[{"payload_json": {}, "occurred_at": ""}]),
            patch(f"{SR}.load_broker", return_value=None),
            patch(f"{SR}.get_connection") as conn,
            patch("symeraseme.llm.factory.create_llm_client") as mock_create,
            patch(f"{SR}.ReplyClassifier") as mock_cls,
            patch(f"{SR}.insert_inbox_reply"),
            patch(f"{SR}.append_event_and_project"),
        ):
            conn.return_value.execute.return_value.fetchone.return_value = {
                "id": 1,
                "subject": "Re: test",
                "snippet": "body",
                "from_addr": "b@b.com",
            }
            mock_cls_instance = mock_cls.return_value
            mock_cls_instance.is_available.return_value = True
            mock_cls_instance.classify.return_value.label = "ack"
            mock_cls_instance.classify.return_value.event_type = "ACK"
            mock_cls_instance.classify.return_value.confidence = 0.95
            mock_cls_instance.classify.return_value.summary = "OK"
            mock_cls_instance.classify.return_value.needs_human_review = False
            mock_cls_instance.classify.return_value.extracted_fields = {}
            mock_cls_instance.classify.return_value.usage_record = None

            handle_classify_reply(request_id=1)

            mock_create.assert_called_once_with(provider=None, model=None)


class TestHandleGenerateRebuttal:
    def test_calls_create_llm_client(self):
        with (
            patch(f"{SR}._ensure_llm_consent", return_value=None),
            patch(f"{SR}.get_removal_request", return_value={"broker_id": "test"}),
            patch(f"{SR}.get_events", return_value=[{"payload_json": {}, "occurred_at": ""}]),
            patch(f"{SR}.load_broker", return_value=None),
            patch(f"{SR}.get_connection") as conn,
            patch("symeraseme.llm.factory.create_llm_client") as mock_create,
            patch(f"{SR}.generate_rebuttal") as mock_gen,
            patch(f"{SR}.profile_exists", return_value=False),
            patch(f"{SR}.append_event_and_project"),
        ):
            conn.return_value.execute.return_value.fetchone.return_value = {
                "id": 1,
                "subject": "Re: test",
                "snippet": "body",
                "from_addr": "b@b.com",
            }
            mock_gen.return_value.label = "test"
            mock_gen.return_value.jurisdiction = "GDPR"
            mock_gen.return_value.rejection_classification = "other"
            mock_gen.return_value.confidence = 0.0
            mock_gen.return_value.needs_human_review = True
            mock_gen.return_value.llm_used = False
            mock_gen.return_value.rebuttal_body = "body"
            mock_gen.return_value.template_name = "template"
            mock_gen.return_value.usage_record = None

            handle_generate_rebuttal(request_id=1, provider="openai", model="gpt-4o")

            mock_create.assert_called_once_with(provider="openai", model="gpt-4o")
