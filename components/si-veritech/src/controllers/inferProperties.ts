import { Context } from "koa";
import api from "@opentelemetry/api";

import Debug from "debug";
const debug = Debug("veritech:controllers:inferProperties");

import { SiEntity as Entity } from "si-entity";
import { registry } from "si-registry";

import intel from "../intel";
import { DecryptedSecret } from "../support";

export interface InferPropertiesRequestContextEntry {
  entity: Entity;
  secret: Record<string, DecryptedSecret | null>;
}

export interface InferPropertiesRequest {
  entityType: string;
  entity: Entity;
  context: InferPropertiesRequestContextEntry[];
}

export interface InferPropertiesReply {
  entity: Entity;
}

export interface InferPropertiesResult {
  entity: Entity;
}

export function inferProperties(ctx: Context): void {
  const span = api.trace.getSpan(api.context.active());
  span.updateName("veritech.inferproperties");
  debug("request body: %O", ctx.request.body);
  const request: InferPropertiesRequest = ctx.request.body;
  span.setAttributes({
    "si.entity.type": request.entityType,
    "si.entity.id": request.entity.id,
  });

  const registryObj = registry[request.entityType];
  // NOTE: This whole thing is going to disappear when we're done refactoring. This is
  // just going to make it work, for now.
  if (!registryObj) {
    ctx.response.status = 200;
    ctx.response.body = { entity: request.entity };
    return;
  }

  //if (!registryObj) {
  //  ctx.response.status = 400;
  //  ctx.response.body = {
  //    code: 400,
  //    message: `Cannot find registry entry for ${request.entityType}`,
  //  };
  //  return;
  //}

  request.entity = Entity.fromJson(request.entity);
  request.entity.setDefaultProperties();
  for (let x = 0; x < request.context.length; x++) {
    request.context[x].entity = Entity.fromJson(request.context[x].entity);
  }

  // Check if this object has the right intel functions
  if (intel[request.entityType] && intel[request.entityType].inferProperties) {
    const result = intel[request.entityType].inferProperties(request);
    result.entity.computeProperties();
    debug("response body: %O", result);
    ctx.response.body = result;
  } else {
    debug("default response");
    request.entity.computeProperties();
    ctx.response.status = 200;
    ctx.response.body = { entity: request.entity };
  }
}
