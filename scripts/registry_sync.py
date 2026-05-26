#!/usr/bin/env python3
"""
Registry synchronization script for Symaira EraseMe.

Scans official data broker registries (California, Vermont) and compares
against the local YAML registry. Can generate new broker definitions and
validate existing ones.

Usage:
    python scripts/registry_sync.py --scan-all --output results.json
    python scripts/registry_sync.py --generate-yaml --input results.json --output-dir registry/brokers/us
    python scripts/registry_sync.py --validate-all
"""

import argparse
import csv
import json
import os
import re
import sys
from datetime import datetime
from pathlib import Path
from typing import Any

import yaml

# Registry URLs and configurations
REGISTRY_SOURCES = {
    "california_2026": {
        "name": "California Data Broker Registry 2026",
        "url": "https://cppa.ca.gov/data-brokers/",
        "csv_path": None,  # Downloaded manually or via API
    },
    "california_legacy": {
        "name": "California Data Broker Registry",
        "url": "https://cppa.ca.gov/data-brokers/",
        "csv_path": None,
    },
    "vermont": {
        "name": "Vermont Data Broker Registry",
        "url": "https://scc.vermont.gov/data-broker-registry",
        "csv_path": None,
    },
}


def normalize_name(name: str) -> str:
    """Normalize broker name for comparison."""
    if not name:
        return ""
    name = name.lower()
    name = re.sub(r'[,\.\(\)]', '', name)
    name = re.sub(r'\s+', ' ', name)
    name = name.strip()
    return name


def generate_id(name: str) -> str:
    """Generate a broker ID from the company name."""
    name = name.lower()
    name = re.sub(r'\s+(llc|inc\.?|corp\.?|corporation|company|co\.?|ltd\.?|limited|gmbh|ag|sa|bv|plc|lp)\s*$', '', name, flags=re.IGNORECASE)
    name = re.sub(r'[,\.\(\)]', '', name)
    name = re.sub(r'\s+', '-', name)
    name = re.sub(r'\band\b', 'and', name)
    name = re.sub(r'[^a-z0-9-]', '', name)
    name = re.sub(r'-+', '-', name)
    name = name.strip('-')
    if not name:
        name = "unknown"
    return f"{name}-us"


def clean_website(url: str) -> str:
    """Ensure website URL has scheme."""
    if not url:
        return ""
    url = url.strip()
    if not url.startswith('http'):
        url = 'https://' + url
    return url


def clean_email(email: str) -> str:
    """Clean and normalize email address."""
    if not email:
        return ""
    email = email.strip()
    email = re.sub(r'\s*\[at\]\s*', '@', email)
    email = email.rstrip('.')
    return email


def load_existing_brokers(registry_dir: str) -> dict[str, dict]:
    """Load all existing broker YAML files."""
    brokers = {}
    for root, _, files in os.walk(registry_dir):
        for filename in files:
            if not filename.endswith('.yaml') or filename.startswith('_'):
                continue
            filepath = os.path.join(root, filename)
            with open(filepath, 'r', encoding='utf-8') as f:
                data = yaml.safe_load(f)
            if data and isinstance(data, dict) and 'name' in data:
                norm = normalize_name(data['name'])
                brokers[norm] = data
    return brokers


def scan_csv_file(csv_path: str, source_name: str, name_col: str, website_col: str, email_col: str) -> list[dict]:
    """Scan a CSV file for broker entries."""
    brokers = []
    try:
        with open(csv_path, 'r', encoding='utf-8') as f:
            reader = csv.DictReader(f)
            for row in reader:
                name = row.get(name_col, '').strip()
                website = row.get(website_col, '').strip()
                email = row.get(email_col, '').strip()
                if name and website:
                    brokers.append({
                        'name': name,
                        'website': clean_website(website),
                        'email': clean_email(email),
                        'source': source_name,
                    })
    except FileNotFoundError:
        print(f"Warning: CSV file not found: {csv_path}", file=sys.stderr)
    return brokers


def scan_all_registries(downloads_dir: str) -> dict[str, Any]:
    """Scan all known registry CSV files."""
    results = {
        'scan_date': datetime.now().isoformat(),
        'sources': [],
        'new_brokers': [],
        'existing_brokers': [],
        'potential_duplicates': [],
    }
    
    registry_dir = Path(__file__).parent.parent / 'registry' / 'brokers'
    existing = load_existing_brokers(str(registry_dir))
    
    downloads_path = Path(downloads_dir)
    
    # Define CSV files to scan
    csv_files = [
        (downloads_path / 'California Data Broker Registry 2026.csv', 'California Data Broker Registry 2026', 'Data broker name:', 'Data broker primary website:', 'Data broker primary contact email address:'),
        (downloads_path / 'ca-data-brokers.csv', 'California Data Broker Registry', 'Data Broker Name', 'Website URL', 'Email Address'),
        (downloads_path / 'vt-data-brokers.csv', 'Vermont Data Broker Registry', 'Data Broker Name:', 'Primary Internet Address:', 'Email Address:'),
        (downloads_path / 'data_brokers_sample.csv', 'Data Broker Sample List', 'name', 'website', 'email'),
    ]
    
    seen = set()
    
    for csv_path, source_name, name_col, website_col, email_col in csv_files:
        if not csv_path.exists():
            print(f"Skipping missing file: {csv_path}", file=sys.stderr)
            continue
        
        results['sources'].append({
            'name': source_name,
            'file': str(csv_path.name),
            'scanned': True,
        })
        
        brokers = scan_csv_file(str(csv_path), source_name, name_col, website_col, email_col)
        
        for broker in brokers:
            norm = normalize_name(broker['name'])
            if norm in seen:
                continue
            seen.add(norm)
            
            if norm in existing:
                results['existing_brokers'].append({
                    'name': broker['name'],
                    'source': source_name,
                    'existing_id': existing[norm].get('id'),
                })
            else:
                results['new_brokers'].append(broker)
    
    return results


def generate_broker_yaml(broker: dict, output_dir: str) -> str:
    """Generate a YAML broker definition file."""
    broker_id = generate_id(broker['name'])
    
    yaml_content = {
        'id': broker_id,
        'name': broker['name'],
        'website': broker['website'],
        'category': 'other',
        'jurisdictions': ['US'],
        'laws': ['CCPA'],
        'data_sensitivity': 3,
        'priority': 'medium',
        'added_date': datetime.now().strftime('%Y-%m-%d'),
        'status': 'active',
    }
    
    if broker.get('email'):
        yaml_content['opt_out'] = [{
            'type': 'email',
            'endpoint': broker['email'],
            'template': 'ccpa-deletion',
            'locale': 'en',
            'required_fields': ['full_name', 'email'],
            'supports_suppression': True,
            'expected_response_days': 45,
        }]
    else:
        yaml_content['opt_out'] = [{
            'type': 'web_form',
            'url': broker['website'],
            'form_spec': {
                'steps': [
                    {'goto': '.'},
                    {'wait_seconds': 5},
                    {'screenshot': 'result'}
                ],
                'timeout_seconds': 30.0,
                'rate_limit_delay': 2.0
            }
        }]
    
    yaml_content['verification'] = {
        'ack_keywords': ['received', 'request', 'submitted'],
        'rejection_keywords': ['cannot', 'denied', 'rejected'],
        'human_required_keywords': ['verify', 'identification', 'upload'],
    }
    
    if broker.get('source'):
        yaml_content['source'] = broker['source']
    
    output_path = Path(output_dir) / f"{broker_id}.yaml"
    with open(output_path, 'w', encoding='utf-8') as f:
        yaml.dump(yaml_content, f, default_flow_style=False, sort_keys=False, allow_unicode=True)
    
    return str(output_path)


def validate_all_brokers(registry_dir: str, schema_path: str) -> bool:
    """Validate all broker YAML files against the JSON schema."""
    try:
        from jsonschema import validate, ValidationError
    except ImportError:
        print("Error: jsonschema not installed. Run: pip install jsonschema", file=sys.stderr)
        return False
    
    with open(schema_path, 'r') as f:
        schema = json.load(f)
    
    valid = 0
    invalid = 0
    
    for root, _, files in os.walk(registry_dir):
        for filename in files:
            if not filename.endswith('.yaml') or filename.startswith('_'):
                continue
            filepath = os.path.join(root, filename)
            with open(filepath, 'r', encoding='utf-8') as f:
                content = f.read()
            try:
                data = yaml.safe_load(content)
                if not data:
                    invalid += 1
                    continue
                validate(instance=data, schema=schema)
                valid += 1
            except Exception as e:
                print(f"Invalid: {filename} - {e}", file=sys.stderr)
                invalid += 1
    
    print(f"Validation: {valid} valid, {invalid} invalid")
    return invalid == 0


def main():
    parser = argparse.ArgumentParser(description='Symaira EraseMe Registry Sync')
    parser.add_argument('--scan-all', action='store_true', help='Scan all registry CSV files')
    parser.add_argument('--output', help='Output file for scan results (JSON)')
    parser.add_argument('--generate-yaml', action='store_true', help='Generate YAML files from scan results')
    parser.add_argument('--input', help='Input file with scan results (JSON)')
    parser.add_argument('--output-dir', default='registry/brokers/us', help='Output directory for YAML files')
    parser.add_argument('--validate-all', action='store_true', help='Validate all existing broker YAML files')
    parser.add_argument('--downloads-dir', default='./downloads/databroker', help='Directory containing registry CSV files')
    
    args = parser.parse_args()
    
    project_root = Path(__file__).parent.parent
    registry_dir = project_root / 'registry' / 'brokers'
    schema_path = project_root / 'registry' / 'schemas' / 'broker.schema.json'
    
    if args.scan_all:
        results = scan_all_registries(args.downloads_dir)
        
        if args.output:
            with open(args.output, 'w') as f:
                json.dump(results, f, indent=2)
            print(f"Scan results written to {args.output}")
        
        print(f"\nScan complete:")
        print(f"  New brokers: {len(results['new_brokers'])}")
        print(f"  Existing: {len(results['existing_brokers'])}")
        print(f"  Sources: {len(results['sources'])}")
    
    if args.generate_yaml:
        if not args.input:
            print("Error: --input required with --generate-yaml", file=sys.stderr)
            sys.exit(1)
        
        with open(args.input, 'r') as f:
            results = json.load(f)
        
        output_dir = Path(args.output_dir)
        output_dir.mkdir(parents=True, exist_ok=True)
        
        generated = []
        for broker in results.get('new_brokers', []):
            path = generate_broker_yaml(broker, str(output_dir))
            generated.append(path)
        
        print(f"Generated {len(generated)} new broker YAML files in {output_dir}")
    
    if args.validate_all:
        success = validate_all_brokers(str(registry_dir), str(schema_path))
        sys.exit(0 if success else 1)


if __name__ == '__main__':
    main()
