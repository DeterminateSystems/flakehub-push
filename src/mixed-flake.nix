{
  inputs.flake.url = "c9026fc0-ced9-48e0-aa3c-fc86c4c86df1";
  outputs = inputs:
    {
      includeOutputPaths = INCLUDE_OUTPUT_PATHS;

      contents =
        let
          getFlakeOutputs = flake:
            let

              # Helper functions.

              mapAttrsToList = f: attrs: map (name: f name attrs.${name}) (builtins.attrNames attrs);

              try = e: default:
                let res = builtins.tryEval e;
                in if res.success then res.value else default;

              mkChildren = children: { inherit children; };

            in

            rec {

              allSchemas = (flake.outputs.schemas or defaultSchemas) // schemaOverrides;

              # FIXME: make this configurable
              defaultSchemas = (builtins.getFlake "https://api.flakehub.com/f/pinned/DeterminateSystems/flake-schemas/0.1.3/0190b841-54d3-7b7a-8550-24942bc38caf/source.tar.gz?narHash=sha256-c2AZH9cOnSpPXV8Lwy19/I8EgW7G%2BE%2BZh6YQBZZwzxI%3D").schemas;

              # Ignore legacyPackages for now, since it's very big and throws uncatchable errors.
              schemaOverrides.legacyPackages = {
                version = 1;
                doc = ''
                  The `legacyPackages` flake output is similar to `packages`, but it can be nested (i.e. contain attribute sets that contain more packages).
                  Since enumerating the packages in nested attribute sets is inefficient, `legacyPackages` should be avoided in favor of `packages`.

                  Note: the contents of `legacyPackages` are not shown in FlakeHub.
                '';
                inventory = output: mkChildren { };
              };

              schemas =
                builtins.listToAttrs (builtins.concatLists (mapAttrsToList
                  (outputName: output:
                    if allSchemas ? ${outputName} then
                      [{ name = outputName; value = allSchemas.${outputName}; }]
                    else
                      [ ])
                  flake.outputs));

              docs =
                builtins.mapAttrs (outputName: schema: schema.doc or "<no docs>") schemas;

              uncheckedOutputs =
                builtins.filter (outputName: ! schemas ? ${outputName}) (builtins.attrNames flake.outputs);

              inventoryFor = filterFun:
                builtins.mapAttrs
                  (outputName: schema:
                    let
                      doFilter = attrs:
                        if filterFun attrs
                        then
                          if attrs ? children
                          then
                            mkChildren (builtins.mapAttrs (childName: child: doFilter child) attrs.children)
                          else
                            {
                              forSystems = attrs.forSystems or null;
                              shortDescription = attrs.shortDescription or null;
                              what = attrs.what or null;
                              #evalChecks = attrs.evalChecks or {};
                            } // (
                              if inputs.self.includeOutputPaths then
                                {
                                  derivation =
                                    if attrs ? derivation
                                    then builtins.unsafeDiscardStringContext attrs.derivation.drvPath
                                    else null;
                                  outputs =
                                    if attrs ? derivation
                                    then
                                      builtins.listToAttrs
                                        (
                                          builtins.map
                                            (outputName:
                                              {
                                                name = outputName;
                                                value = attrs.derivation.${outputName}.outPath;
                                              }
                                            )
                                            attrs.derivation.outputs
                                        )
                                    else
                                      null;
                                }
                              else
                                { }
                            )
                        else
                          { };
                    in
                    doFilter ((schema.inventory or (output: { })) flake.outputs.${outputName})
                  )
                  schemas;

              inventoryForSystem = system: inventoryFor (itemSet:
                !itemSet ? forSystems
                || builtins.any (x: x == system) itemSet.forSystems);

              inventory = inventoryFor (x: true);

              contents = {
                version = 1;
                inherit docs;
                inherit inventory;
              };

            };
        in
        (getFlakeOutputs inputs.flake).contents;
    };
}
