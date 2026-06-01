from __future__ import annotations

from datetime import date
from enum import StrEnum

from pydantic import BaseModel, EmailStr, Field


class BrokerCategory(StrEnum):
    people_search = "people-search"
    marketing = "marketing"
    credit = "credit"
    analytics = "analytics"
    background_check = "background-check"
    social_media = "social-media"
    other = "other"


class Law(StrEnum):
    gdpr = "GDPR"
    ccpa = "CCPA"
    cpra = "CPRA"
    lgpd = "LGPD"
    pipeda = "PIPEDA"


class Priority(StrEnum):
    high = "high"
    medium = "medium"
    low = "low"


class SolveCaptcha(BaseModel):
    type: str
    site_key: str
    provider: str | None = None
    action: str | None = None
    min_score: float | None = None
    is_invisible: bool | None = None


class FormStep(BaseModel):
    goto: str | None = None
    fill: dict[str, str] | None = None
    click: str | None = None
    select: dict[str, str] | None = None
    wait_for: str | None = None
    wait_seconds: float | None = None
    screenshot: str | None = None
    assert_text: str | None = None
    solve_captcha: SolveCaptcha | None = None


class FormSpec(BaseModel):
    steps: list[FormStep]
    timeout_seconds: float = 30.0
    headless: bool = True
    rate_limit_delay: float = 1.0


class EmailOptOut(BaseModel):
    type: str = "email"
    endpoint: EmailStr
    template: str
    locale: str
    required_fields: list[str] = Field(default_factory=list)
    supports_suppression: bool = False
    expected_response_days: int = 30


class WebFormOptOut(BaseModel):
    type: str = "web_form"
    url: str
    form_spec: FormSpec


OptOutChannel = EmailOptOut | WebFormOptOut


class Verification(BaseModel):
    ack_keywords: list[str] = Field(default_factory=list)
    rejection_keywords: list[str] = Field(default_factory=list)
    human_required_keywords: list[str] = Field(default_factory=list)


class Broker(BaseModel):
    """A data broker with opt-out channels, jurisdiction and verification rules."""

    id: str = Field(pattern=r"^[a-z0-9-]+$")
    name: str
    website: str
    category: BrokerCategory
    jurisdictions: list[str] = Field(min_length=1)
    laws: list[Law] = Field(min_length=1)
    data_sensitivity: int = Field(default=3, ge=1, le=5)
    priority: Priority
    opt_out: list[OptOutChannel] = Field(min_length=1)
    verification: Verification | None = None
    disabled: bool = False
    notes: str | None = None


class Address(BaseModel):
    street: str
    city: str
    postal_code: str
    country: str
    valid_from: date | None = None
    valid_to: date | None = None


class IdentityProfile(BaseModel):
    full_name: str
    name_variants: list[str] = Field(default_factory=list)
    date_of_birth: date | None = None
    addresses: list[Address] = Field(default_factory=list)
    email_addresses: list[EmailStr] = Field(default_factory=list)
    phone_numbers: list[str] = Field(default_factory=list)
    jurisdictions: list[str] = Field(default_factory=list)
