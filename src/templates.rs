pub const MAIN_PAGE_TEMPLATE: &str = r#"
                  <!DOCTYPE html>
                    <html>
                      <head>
                        <title>Warp Handlebars template example</title>
                        <style>
                        body {
                          font-family: "Helvetica Neue", Helvetica, Arial;
                          font-size: 14px;
                          line-height: 20px;
                          font-weight: 400;
                          color: #3b3b3b;
                          -webkit-font-smoothing: antialiased;
                          font-smoothing: antialiased;
                          background: #2b2b2b;
                        }
                        @media screen and (max-width: 580px) {
                          body {
                            font-size: 16px;
                            line-height: 22px;
                          }
                        }
                        
                        .wrapper {
                          margin: 0 auto;
                          padding: 40px;
                          max-width: 800px;
                        }
                        
                        .table {
                          margin: 0 0 40px 0;
                          width: 100%;
                          box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
                          display: table;
                        }
                        @media screen and (max-width: 580px) {
                          .table {
                            display: block;
                          }
                        }
                        
                        .row {
                          display: table-row;
                          background: #f6f6f6;
                        }
                        .row:nth-of-type(odd) {
                          background: #e9e9e9;
                        }
                        .row.header {
                          font-weight: 900;
                          color: #ffffff;
                          background: #ea6153;
                        }
                        .row.green {
                          background: #27ae60;
                        }
                        .row.blue {
                          background: #2980b9;
                        }
                        @media screen and (max-width: 580px) {
                          .row {
                            padding: 14px 0 7px;
                            display: block;
                          }
                          .row.header {
                            padding: 0;
                            height: 6px;
                          }
                          .row.header .cell {
                            display: none;
                          }
                          .row .cell {
                            margin-bottom: 10px;
                          }
                          .row .cell:before {
                            margin-bottom: 3px;
                            content: attr(data-title);
                            min-width: 98px;
                            font-size: 10px;
                            line-height: 10px;
                            font-weight: bold;
                            text-transform: uppercase;
                            color: #969696;
                            display: block;
                          }
                        }
                        
                        .cell {
                          padding: 6px 12px;
                          display: table-cell;
                        }
                        @media screen and (max-width: 580px) {
                          .cell {
                            padding: 2px 16px;
                            display: block;
                          }
                        }
                        </style>
                      </head>
                      <body>
                         <div class="table">
                        
                            <div class="row header">
                              <div class="cell">
                               Path Pattern 
                              </div>
                              <div class="cell">
                               Container ID 
                              </div>
                              <div class="cell">
                                Checksum 
                              </div>
                              <div class="cell">
                                Created At 
                              </div>
                              <div class="cell">
                                Transmitted At 
                              </div>
                            </div>
                        
                            {{#each files}}
                            <div class="row">
                              <div class="cell" data-title="Path Pattern">
                                {{pattern}} 
                              </div>
                              <div class="cell" data-title="Container ID">
                                {{container_id}}
                              </div>
                              <div class="cell" data-title="Checksum">
                                {{checksum}}
                              </div>
                              <div class="cell" data-title="Created At">
                                {{created_at}}
                              </div>
                              <div class="cell" data-title="Transmitted At">
                                {{transmitted_at}}
                              </div>
                            </div>
                            {{/each}}
                          </div>
                      </body>
                    </html>"#;
