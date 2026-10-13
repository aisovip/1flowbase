import type {
  FlowNodeDocument,
  FlowNodeOutputDocument,
  NamedBindingEntry
} from '@1flowbase/flow-schema';
import { i18nText } from '../../../../../../shared/i18n/text';
import type { NodeDefinition } from '../../types';
import { basicFields } from '../../base';

export const notificationNodeDefinition: NodeDefinition = {
  label: i18nText('agentFlow', 'auto.notification_node'),
  sections: [
    { key: 'basics', title: 'Basics', fields: basicFields },
    {
      key: 'inputs',
      title: 'Inputs',
      fields: [
        {
          key: 'config.instance_id',
          label: i18nText('agentFlow', 'auto.notification_instance'),
          editor: 'notification',
          required: true
        }
      ]
    },
    {
      key: 'outputs',
      title: 'Outputs',
      fields: [
        {
          key: 'config.output_contract',
          label: i18nText('agentFlow', 'auto.output_variable'),
          editor: 'output_contract_definition'
        }
      ]
    }
  ]
};
export function notificationNodeOutputs(
  output_schema: Record<string, unknown> = { type: 'object' }
): FlowNodeOutputDocument[] {
  return [
    {
      key: 'result',
      title: 'result',
      valueType: 'object',
      selector: ['result'],
      jsonSchema: output_schema
    }
  ];
}
/** Instance identity is the only durable configuration copied into the flow. */
export function selectNotificationInstance(
  node: FlowNodeDocument,
  instance_id: string,
  output_schema: Record<string, unknown>,
  input: NamedBindingEntry[] = []
): FlowNodeDocument {
  if (node.config.instance_id === instance_id) return node;
  return {
    ...node,
    config: { ...node.config, instance_id },
    bindings: { input: { kind: 'named_bindings', value: input } },
    outputs: notificationNodeOutputs(output_schema)
  };
}

/** Schema properties expose selectors inside the real result envelope without
 * persisting extra output aliases or flattening the runtime output. */
export function notificationNestedOutputs(
  output: FlowNodeOutputDocument
): FlowNodeOutputDocument[] {
  function visit(
    schema: Record<string, unknown>,
    path: string[]
  ): FlowNodeOutputDocument[] {
    const properties = schema.properties;
    if (
      typeof properties !== 'object' ||
      properties === null ||
      Array.isArray(properties)
    )
      return [];
    return Object.entries(properties).flatMap(([key, value]) => {
      if (typeof value !== 'object' || value === null || Array.isArray(value))
        return [];
      const child = value as Record<string, unknown>;
      const selector = [...path, key];
      const type = typeof child.type === 'string' ? child.type : 'json';
      return [
        {
          key: selector.join('.'),
          title: selector.join('.'),
          valueType: type === 'integer' ? 'number' : type,
          selector,
          jsonSchema: child
        },
        ...visit(child, selector)
      ];
    });
  }
  return output.jsonSchema
    ? visit(output.jsonSchema, output.selector ?? [output.key])
    : [];
}
