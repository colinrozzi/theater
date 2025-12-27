#!/usr/bin/env python3
"""
Script to convert handler crates from old event recording pattern to new pattern.
"""
import re
import sys
from pathlib import Path

# Mapping of old EventData types to new handler-specific event types
EVENT_TYPE_MAPPING = {
    'EventData::Http': 'HttpFrameworkEventData',
    'EventData::Message': 'MessageEventData',
    'EventData::Process': 'ProcessEventData',
    'EventData::Random': 'RandomEventData',
    'EventData::Store': 'StoreEventData',
    'EventData::Supervisor': 'SupervisorEventData',
}

def convert_event_recording(content, event_data_type):
    """Convert ctx.data_mut().record_event(ChainEventData {}) calls to record_handler_event()"""

    # Pattern to match the old event recording style
    # This handles multi-line ChainEventData structs
    pattern = r'ctx\.data_mut\(\)\.record_event\(ChainEventData\s*\{\s*event_type:\s*([^,]+),\s*data:\s*' + re.escape(event_data_type) + r'\(([^)]+)\),\s*timestamp:[^,]+,\s*description:\s*([^}]+)\}\);'

    def replacer(match):
        event_type = match.group(1).strip()
        event_variant = match.group(2).strip()
        description = match.group(3).strip()

        # Get the event type name without "EventData::"
        handler_event_type = EVENT_TYPE_MAPPING[event_data_type]

        return f'ctx.data_mut().record_handler_event({event_type}, {handler_event_type}::{event_variant}, {description});'

    content = re.sub(pattern, replacer, content, flags=re.MULTILINE | re.DOTALL)

    # Also handle simpler single-line patterns
    pattern2 = r'ctx\.data_mut\(\)\.record_event\(ChainEventData \{\s*event_type: ([^,]+),\s*data: ' + re.escape(event_data_type) + r'\(([^)]+)\),\s*timestamp: [^,]+,\s*description: ([^}]+)\}\);'
    content = re.sub(pattern2, replacer, content, flags=re.MULTILINE)

    return content

def convert_actor_store_events(content, event_data_type):
    """Convert actor_component.actor_store.record_event() calls"""
    handler_event_type = EVENT_TYPE_MAPPING[event_data_type]

    pattern = r'actor_component\.actor_store\.record_event\(ChainEventData\s*\{\s*event_type:\s*([^,]+),\s*data:\s*' + re.escape(event_data_type) + r'\(([^)]+)\),\s*timestamp:[^,]+,\s*description:\s*([^}]+)\}\);'

    def replacer(match):
        event_type = match.group(1).strip()
        event_variant = match.group(2).strip()
        description = match.group(3).strip()

        return f'actor_component.actor_store.record_handler_event({event_type}, {handler_event_type}::{event_variant}, {description});'

    content = re.sub(pattern, replacer, content, flags=re.MULTILINE | re.DOTALL)
    return content

def update_handler_file(file_path, event_data_type):
    """Update a single handler file"""
    print(f"Processing {file_path}...")

    with open(file_path, 'r') as f:
        content = f.read()

    # Convert event recordings
    content = convert_event_recording(content, event_data_type)
    content = convert_actor_store_events(content, event_data_type)

    # Update Handler trait implementation signature
    content = re.sub(
        r'fn setup_host_functions\(&mut self, actor_component: &mut ActorComponent\) -> Result<\(\)>',
        r'fn setup_host_functions(&mut self, actor_component: &mut ActorComponent<E>) -> Result<()>',
        content
    )

    content = re.sub(
        r'fn add_export_functions\(&self, actor_instance: &mut ActorInstance\) -> Result<\(\)>',
        r'fn add_export_functions(&self, actor_instance: &mut ActorInstance<E>) -> Result<()>',
        content
    )

    # Remove old imports if they exist
    content = re.sub(
        r'use theater::events::\{ChainEventData, EventData\};\n',
        '',
        content
    )

    with open(file_path, 'w') as f:
        f.write(content)

    print(f"✓ Updated {file_path}")

def main():
    # Map of handler crates to their event types
    handlers = {
        'theater-handler-http-framework': 'EventData::Http',
        'theater-handler-http-client': 'EventData::Http',
        'theater-handler-message-server': 'EventData::Message',
        'theater-handler-process': 'EventData::Process',
        'theater-handler-random': 'EventData::Random',
        'theater-handler-store': 'EventData::Store',
        'theater-handler-supervisor': 'EventData::Supervisor',
    }

    base_path = Path('/Users/colinrozzi/work/theater/crates')

    for handler_name, event_type in handlers.items():
        lib_path = base_path / handler_name / 'src' / 'lib.rs'
        if lib_path.exists():
            update_handler_file(lib_path, event_type)
        else:
            print(f"⚠ File not found: {lib_path}")

    print("\n✅ All handlers updated!")

if __name__ == '__main__':
    main()
