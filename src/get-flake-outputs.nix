flake:

let

  # Helper functions.

  mapAttrsToList = f: attrs: map (name: f name attrs.${name}) (builtins.attrNames attrs);

  try = e: default:
    let res = builtins.tryEval e;
    in if res.success then res.value else default;

  mkChildren = children: { inherit children; };

  mkLeaf = leaf: { inherit leaf; };

in

rec {

  allSchemas = (flake.outputs.schemas or defaultSchemas) // schemaOverrides;

  # FIXME: make this configurable
  defaultSchemas = (builtins.getFlake "https://api.flakehub.com/f/pinned/DeterminateSystems/flake-schemas/0.0.6+rev-8b320d78bfbc47515f3366b43e1a29c69519f36a/0189d657-3901-716b-9cde-7ec5aacc7cf8/source.tar.gz?narHash=sha256-3H1AXsE4SRqT1tZDb2gxdSwf3JInOgS1F8nq2QsXmHg=").schemas;

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
              else if attrs ? leaf then
                mkLeaf
                  {
                    forSystems = attrs.leaf.forSystems or null;
                    doc = if attrs.leaf ? doc then try attrs.leaf.doc "«evaluation error»" else null;
                    #evalChecks = attrs.leaf.evalChecks or {};
                  }
              else
                throw "Schema returned invalid tree node."
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

}
