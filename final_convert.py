#!/usr/bin/env python3
"""
Robust converter for handler event recording patterns.
Handles all edge cases including nested braces, multiline strings, etc.
"""
import re
from pathlib import Path

def extract_event_blocks(content):
    """Extract all ChainEventData blocks with their positions."""
    blocks = []
    patterns = [
        r'ctx\.data_mut\(\)\.record_event\(',
        r'actor_component\.actor_store\.record_event\('
    ]

    for pattern in patterns:
        for match in re.finditer(pattern, content):
            start = match.start()
            pos = match.end()

            # Check if this is ChainEventData
            if content[pos:pos+15] == 'ChainEventData ':
                # Find the matching closing );
                brace_count = 0
                paren_count = 1  # We're inside the record_event(
                in_string = False
                escape_next = False

                while pos < len(content) and paren_count > 0:
                    char = content[pos]

                    if escape_next:
                        escape_next = False
                    elif char == '\\':
                        escape_next = True
                    elif char == '"' and not in_string:
                        in_string = True
                    elif char == '"' and in_string:
                        in_string = False
                    elif not in_string:
                        if char == '{':
                            brace_count += 1
                        elif char == '}':
                            brace_count -= 1
                        elif char == '(':
                            paren_count += 1
                        elif char == ')':
                            paren_count -= 1

                    pos += 1

                # Find the semicolon
                while pos < len(content) and content[pos] in ' \t\n':
                    pos += 1
                if pos < len(content) and content[pos] == ';':
                    pos += 1
                    blocks.append((start, pos))

    return blocks

def parse_chain_event(text):
    """Parse a ChainEventData block."""
    # Determine prefix
    if text.startswith('ctx.data_mut()'):
        prefix = 'ctx.data_mut()'
    else:
        prefix = 'actor_component.actor_store'

    # Extract event_type
    event_type_match = re.search(r'event_type:\s*(.+?),\s*data:', text, re.DOTALL)
    if not event_type_match:
        return None
    event_type = event_type_match.group(1).strip()

    # Extract description
    desc_match = re.search(r'description:\s*(.+?)\s*\}\s*\)\s*;?\s*$', text, re.DOTALL)
    if not desc_match:
        return None
    description = desc_match.group(1).strip()

    # Extract data - find the EventData:: part
    data_patterns = [
        (r'EventData::Http\((.+?)\),\s*timestamp:', 'HttpEventData', 'HttpFrameworkEventData'),
        (r'EventData::Message\((.+?)\),\s*timestamp:', 'MessageEventData', 'MessageEventData'),
        (r'EventData::Process\((.+?)\),\s*timestamp:', 'ProcessEventData', 'ProcessEventData'),
        (r'EventData::Random\((.+?)\),\s*timestamp:', 'RandomEventData', 'RandomEventData'),
        (r'EventData::Store\((.+?)\),\s*timestamp:', 'StoreEventData', 'StoreEventData'),
        (r'EventData::Supervisor\((.+?)\),\s*timestamp:', 'SupervisorEventData', 'SupervisorEventData'),
    ]

    event_data = None
    new_type = None
    for pattern, http_type, other_type in data_patterns:
        match = re.search(pattern, text, re.DOTALL)
        if match:
            event_data = match.group(1).strip()
            # For http handlers, check if it's framework or client
            if 'EventData::Http' in pattern:
                if 'http-framework' in text or 'theater:simple/http-framework' in text:
                    new_type = 'HttpFrameworkEventData'
                else:
                    new_type = 'HttpEventData'
            else:
                new_type = other_type
            break

    if not event_data or not new_type:
        return None

    return {
        'prefix': prefix,
        'event_type': event_type,
        'event_data': event_data,
        'description': description,
        'new_type': new_type
    }

def convert_handler(filepath):
    """Convert a single handler file."""
    with open(filepath, 'r') as f:
        content = f.read()

    original_content = content

    # Find all event blocks
    blocks = extract_event_blocks(content)

    # Process from end to start to maintain positions
    for start, end in reversed(blocks):
        block_text = content[start:end]
        parsed = parse_chain_event(block_text)

        if parsed:
            # Build replacement
            indent = len(content[start:start+100].split('\n')[0]) - len(content[start:start+100].split('\n')[0].lstrip())
            replacement = f'{parsed["prefix"]}.record_handler_event({parsed["event_type"]}, {parsed["new_type"]}::{parsed["event_data"]}, {parsed["description"]});'

            content = content[:start] + replacement + content[end:]

    # Update method signatures
    content = re.sub(
        r'fn setup_host_functions\(&mut self, actor_component: &mut ActorComponent\) -> Result<\(\)>',
        'fn setup_host_functions(&mut self, actor_component: &mut ActorComponent<E>) -> Result<()>',
        content
    )

    content = re.sub(
        r'fn add_export_functions\(&self, actor_instance: &mut ActorInstance\) -> Result<\(\)>',
        'fn add_export_functions(&self, actor_instance: &mut ActorInstance<E>) -> Result<()>',
        content
    )

    if content != original_content:
        with open(filepath, 'w') as f:
            f.write(content)
        return True
    return False

def main():
    base = Path('/Users/colinrozzi/work/theater/crates')
    handlers = [
        'theater-handler-http-framework',
        'theater-handler-http-client',
        'theater-handler-message-server',
        'theater-handler-supervisor',
    ]

    for handler in handlers:
        filepath = base / handler / 'src' / 'lib.rs'
        if filepath.exists():
            if convert_handler(filepath):
                print(f'✓ Converted {handler}')
            else:
                print(f'⚠ No changes in {handler}')
        else:
            print(f'⚠ Not found: {filepath}')

if __name__ == '__main__':
    main()
